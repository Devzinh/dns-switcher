//! Resolver measurement over plain DNS (UDP/53).
//!
//! Each probe sends hand-built RFC 1035 queries and reports:
//! - warm samples: repeated A lookups of a popular name (answered from cache),
//!   summarised as median, p95, jitter and loss;
//! - one cold lookup: a random, never-cached name, so the resolver has to
//!   recurse to the authoritative servers;
//! - the anycast site that answered, from a CHAOS TXT `id.server` query
//!   (supported by Cloudflare, Quad9 and others; `None` when unsupported);
//! - answer quality: the resolver is asked for a CDN hostname whose answer
//!   depends on where the CDN thinks you are, and the time to open a TCP
//!   connection to the server it returns is measured. A resolver that answers
//!   fast but sends you to a far-away server shows up here.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub const WARM_DOMAIN: &str = "www.google.com";
pub const COLD_PARENT: &str = "google.com";
/// Hosts whose answer depends on where the CDN thinks the resolver (or the
/// client subnet it forwards) is: Akamai, Azure Front Door/Akamai and
/// CloudFront. Measured from Brazil, resolvers in Europe sent all three to
/// servers ~200 ms away while nearby resolvers got 4–12 ms. Anycast-only
/// sites (Google, Cloudflare, Netflix) answer the same everywhere, so they
/// can't tell resolvers apart and aren't used.
pub const CDN_HOSTS: [&str; 3] = ["www.akamai.com", "www.microsoft.com", "www.amazon.com"];
const CDN_PORT: u16 = 443;
const CDN_CONNECT_MS: u64 = 1000;
/// Added to the score of servers whose CDN answer couldn't be measured when
/// others could, so they sort after them instead of looking fast.
const CDN_MISSING_PENALTY: u32 = 1000;
pub const SAMPLES: usize = 5;
pub const TIMEOUT_MS: u64 = 500;
const GAP_MS: u64 = 80;

const TYPE_A: u16 = 1;
const TYPE_TXT: u16 = 16;
const CLASS_IN: u16 = 1;
const CLASS_CH: u16 = 3;

/// Result of probing one resolver.
#[derive(Clone, Debug, Default)]
pub struct Probe {
    /// Round-trip time of each warm sample in ms; `None` = no valid reply in time.
    pub samples: Vec<Option<u32>>,
    /// Round-trip time of the uncached lookup in ms.
    pub cold: Option<u32>,
    /// Anycast site identifier reported by the resolver, if any.
    pub site: Option<String>,
    /// Median TCP connect time (ms) to the CDN servers this resolver picked.
    pub cdn: Option<u32>,
    /// The CDN servers this resolver returned, with connect times.
    pub cdn_detail: Vec<(&'static str, Ipv4Addr, Option<u32>)>,
}

impl Probe {
    fn answered(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.samples.iter().flatten().copied().collect();
        v.sort_unstable();
        v
    }

    pub fn median(&self) -> Option<u32> {
        let v = self.answered();
        match v.len() {
            0 => None,
            n if n % 2 == 1 => Some(v[n / 2]),
            n => Some((v[n / 2 - 1] + v[n / 2]) / 2),
        }
    }

    /// 95th percentile, nearest-rank method.
    pub fn p95(&self) -> Option<u32> {
        let v = self.answered();
        if v.is_empty() {
            return None;
        }
        let rank = ((0.95 * v.len() as f64).ceil() as usize).max(1);
        Some(v[rank - 1])
    }

    /// Mean absolute difference between consecutive answered samples (RFC 3550 style).
    pub fn jitter(&self) -> Option<u32> {
        let seq: Vec<u32> = self.samples.iter().flatten().copied().collect();
        if seq.len() < 2 {
            return None;
        }
        let sum: u32 = seq.windows(2).map(|w| w[0].abs_diff(w[1])).sum();
        Some((sum as f32 / (seq.len() - 1) as f32).round() as u32)
    }

    /// Share of warm samples that got no valid reply, 0–100.
    pub fn loss_pct(&self) -> u32 {
        if self.samples.is_empty() {
            return 100;
        }
        let lost = self.samples.iter().filter(|s| s.is_none()).count();
        (lost * 100 / self.samples.len()) as u32
    }

    /// Ranking key: reliable servers (≤ 20% loss) first, then by score
    /// (median lookup time, plus the CDN connect time when `use_cdn`), then p95.
    pub fn rank_key(&self, use_cdn: bool) -> Option<(bool, u32, u32)> {
        let m = self.median()?;
        let score = if use_cdn { m + self.cdn.unwrap_or(CDN_MISSING_PENALTY) } else { m };
        Some((self.loss_pct() > 20, score, self.p95().unwrap_or(m)))
    }
}

/// Index of the best probe. When at least one CDN measurement worked, the
/// ranking includes it: what matters is how fast you reach the content, not
/// only how fast the name resolves.
pub fn best(probes: &[Option<Probe>]) -> Option<usize> {
    let use_cdn = probes.iter().flatten().any(|p| p.cdn.is_some());
    probes
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.as_ref()?.rank_key(use_cdn).map(|k| (k, i)))
        .min()
        .map(|(_, i)| i)
}

/// Full measurement of one resolver. Blocking; run it off the UI thread.
pub fn probe(server: &str) -> Probe {
    let Ok(ip) = server.trim().parse::<IpAddr>() else {
        return Probe { samples: vec![None; SAMPLES], ..Default::default() };
    };
    let timeout = Duration::from_millis(TIMEOUT_MS);
    let mut samples = Vec::with_capacity(SAMPLES);
    for i in 0..SAMPLES {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(GAP_MS));
        }
        samples.push(exchange(ip, WARM_DOMAIN, TYPE_A, CLASS_IN, timeout).map(|(ms, _)| ms));
    }
    let cold_name = format!("{}.{}", random_label(), COLD_PARENT);
    let cold = exchange(ip, &cold_name, TYPE_A, CLASS_IN, timeout).map(|(ms, _)| ms);
    let site = exchange(ip, "id.server", TYPE_TXT, CLASS_CH, timeout)
        .and_then(|(_, reply)| first_txt(&reply))
        .filter(|s| !s.trim().is_empty());
    let cdn_detail: Vec<_> = CDN_HOSTS
        .iter()
        .filter_map(|h| resolve_a(ip, h).map(|a| (*h, a, connect_ms(a))))
        .collect();
    let mut times: Vec<u32> = cdn_detail.iter().filter_map(|(_, _, ms)| *ms).collect();
    times.sort_unstable();
    let cdn = times.get(times.len() / 2).copied().filter(|_| !times.is_empty());
    Probe { samples, cold, site, cdn, cdn_detail }
}

/// IPv4 address `server` returns for `host`, or `None`.
pub fn resolve_a(server: IpAddr, host: &str) -> Option<Ipv4Addr> {
    exchange(server, host, TYPE_A, CLASS_IN, Duration::from_millis(TIMEOUT_MS)).and_then(|(_, r)| first_a(&r))
}

/// Best of two TCP handshakes to the CDN server, in ms.
fn connect_ms(ip: Ipv4Addr) -> Option<u32> {
    let addr = SocketAddr::new(IpAddr::V4(ip), CDN_PORT);
    (0..2)
        .filter_map(|_| {
            let start = Instant::now();
            TcpStream::connect_timeout(&addr, Duration::from_millis(CDN_CONNECT_MS))
                .ok()
                .map(|_| start.elapsed().as_millis() as u32)
        })
        .min()
}

/// Sends one query and waits for the matching reply. Returns (ms, reply bytes).
fn exchange(ip: IpAddr, name: &str, qtype: u16, qclass: u16, timeout: Duration) -> Option<(u32, Vec<u8>)> {
    let bind = if ip.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
    let sock = UdpSocket::bind(bind).ok()?;
    sock.connect(SocketAddr::new(ip, 53)).ok()?;
    let id = random_u64() as u16;
    let query = build_query(id, name, qtype, qclass)?;
    let start = Instant::now();
    sock.send(&query).ok()?;
    let mut buf = [0u8; 1232];
    loop {
        let left = timeout.checked_sub(start.elapsed())?;
        sock.set_read_timeout(Some(left.max(Duration::from_millis(1)))).ok()?;
        let n = sock.recv(&mut buf).ok()?;
        if is_reply_to(&buf[..n], id) {
            return Some((start.elapsed().as_millis() as u32, buf[..n].to_vec()));
        }
        // Stray or mismatched datagram: keep waiting within the same budget.
    }
}

/// RFC 1035 query with RD set and one question. `None` if a label is invalid.
pub fn build_query(id: u16, name: &str, qtype: u16, qclass: u16) -> Option<Vec<u8>> {
    let mut q = Vec::with_capacity(18 + name.len());
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00]); // flags: RD
    q.extend_from_slice(&[0x00, 0x01]); // QDCOUNT
    q.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // AN/NS/AR
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&qtype.to_be_bytes());
    q.extend_from_slice(&qclass.to_be_bytes());
    Some(q)
}

/// A response (QR=1) carrying our transaction id.
fn is_reply_to(msg: &[u8], id: u16) -> bool {
    msg.len() >= 12 && u16::from_be_bytes([msg[0], msg[1]]) == id && msg[2] & 0x80 != 0
}

/// The answer section's records as (type, rdata), skipping the question.
fn answers(msg: &[u8]) -> Option<Vec<(u16, &[u8])>> {
    if msg.len() < 12 {
        return None;
    }
    let qd = u16::from_be_bytes([msg[4], msg[5]]) as usize;
    let an = u16::from_be_bytes([msg[6], msg[7]]) as usize;
    let mut pos = 12;
    for _ in 0..qd {
        pos = skip_name(msg, pos)? + 4;
    }
    let mut out = Vec::with_capacity(an);
    for _ in 0..an {
        pos = skip_name(msg, pos)?;
        let head = msg.get(pos..pos + 10)?;
        let rtype = u16::from_be_bytes([head[0], head[1]]);
        let rdlen = u16::from_be_bytes([head[8], head[9]]) as usize;
        pos += 10;
        out.push((rtype, msg.get(pos..pos + rdlen)?));
        pos += rdlen;
    }
    Some(out)
}

/// First IPv4 address in the answer section (after any CNAME chain).
pub fn first_a(msg: &[u8]) -> Option<Ipv4Addr> {
    answers(msg)?
        .into_iter()
        .find(|(t, d)| *t == TYPE_A && d.len() == 4)
        .map(|(_, d)| Ipv4Addr::new(d[0], d[1], d[2], d[3]))
}

/// Text of the first TXT record in the answer section, strings concatenated.
pub fn first_txt(msg: &[u8]) -> Option<String> {
    if msg.len() < 12 {
        return None;
    }
    let qd = u16::from_be_bytes([msg[4], msg[5]]) as usize;
    let an = u16::from_be_bytes([msg[6], msg[7]]) as usize;
    let mut pos = 12;
    for _ in 0..qd {
        pos = skip_name(msg, pos)? + 4;
    }
    for _ in 0..an {
        pos = skip_name(msg, pos)?;
        let head = msg.get(pos..pos + 10)?;
        let rtype = u16::from_be_bytes([head[0], head[1]]);
        let rdlen = u16::from_be_bytes([head[8], head[9]]) as usize;
        pos += 10;
        let rdata = msg.get(pos..pos + rdlen)?;
        pos += rdlen;
        if rtype == TYPE_TXT {
            let mut out = String::new();
            let mut i = 0;
            while i < rdata.len() {
                let len = rdata[i] as usize;
                out.push_str(&String::from_utf8_lossy(rdata.get(i + 1..i + 1 + len)?));
                i += 1 + len;
            }
            return Some(out);
        }
    }
    None
}

/// Position just past a (possibly compressed) domain name.
fn skip_name(msg: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        let len = *msg.get(pos)?;
        match len {
            0 => return Some(pos + 1),
            l if l & 0xC0 == 0xC0 => return Some(pos + 2),
            l => pos += 1 + l as usize,
        }
    }
}

fn random_u64() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    h.write_u128(now.as_nanos());
    h.finish()
}

/// 12 lowercase alphanumerics, e.g. `k3x9q0ab7mzt`.
fn random_label() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut x = random_u64();
    (0..12)
        .map(|_| {
            let c = ALPHABET[(x % 36) as usize] as char;
            x /= 36;
            if x == 0 {
                x = random_u64();
            }
            c
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(samples: &[Option<u32>]) -> Probe {
        Probe { samples: samples.to_vec(), ..Default::default() }
    }

    #[test]
    fn query_matches_rfc1035_layout() {
        let q = build_query(0x1234, "www.google.com", TYPE_A, CLASS_IN).unwrap();
        let expected: Vec<u8> = vec![
            0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0, //
            3, b'w', b'w', b'w', 6, b'g', b'o', b'o', b'g', b'l', b'e', 3, b'c', b'o', b'm', 0, //
            0x00, 0x01, 0x00, 0x01,
        ];
        assert_eq!(q, expected);
    }

    #[test]
    fn query_rejects_bad_labels() {
        assert!(build_query(1, "a..b", TYPE_A, CLASS_IN).is_none());
        assert!(build_query(1, &"x".repeat(64), TYPE_A, CLASS_IN).is_none());
    }

    #[test]
    fn reply_check_needs_id_and_qr() {
        let mut m = vec![0x12, 0x34, 0x81, 0x80, 0, 1, 0, 0, 0, 0, 0, 0];
        assert!(is_reply_to(&m, 0x1234));
        assert!(!is_reply_to(&m, 0x1235));
        m[2] = 0x01; // QR=0: a query, not a reply
        assert!(!is_reply_to(&m, 0x1234));
        assert!(!is_reply_to(&m[..8], 0x1234));
    }

    #[test]
    fn parses_txt_with_compressed_owner() {
        let mut m = vec![0xab, 0xcd, 0x84, 0x00, 0, 1, 0, 1, 0, 0, 0, 0];
        m.extend(build_query(0, "id.server", TYPE_TXT, CLASS_CH).unwrap()[12..].iter());
        m.extend_from_slice(&[0xC0, 0x0C, 0, 16, 0, 3, 0, 0, 0, 0]);
        m.extend_from_slice(&[0, 4, 3, b'G', b'R', b'U']);
        assert_eq!(first_txt(&m).as_deref(), Some("GRU"));
    }

    #[test]
    fn parses_a_after_cname() {
        let mut m = vec![0, 1, 0x81, 0x80, 0, 1, 0, 2, 0, 0, 0, 0];
        m.extend(build_query(0, "www.akamai.com", TYPE_A, CLASS_IN).unwrap()[12..].iter());
        // CNAME www.akamai.com -> x (rdata: 1 'x' 0)
        m.extend_from_slice(&[0xC0, 0x0C, 0, 5, 0, 1, 0, 0, 0, 60, 0, 3, 1, b'x', 0]);
        m.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 23, 45, 67, 89]);
        assert_eq!(first_a(&m), Some(Ipv4Addr::new(23, 45, 67, 89)));
        assert_eq!(first_a(&m[..m.len() - 2]), None);
    }

    #[test]
    fn txt_parser_survives_truncation() {
        let mut m = vec![0, 0, 0x84, 0, 0, 1, 0, 1, 0, 0, 0, 0];
        m.extend(build_query(0, "id.server", TYPE_TXT, CLASS_CH).unwrap()[12..].iter());
        m.extend_from_slice(&[0xC0, 0x0C, 0, 16, 0, 3]);
        assert_eq!(first_txt(&m), None);
    }

    #[test]
    fn stats() {
        let x = p(&[Some(10), Some(30), None, Some(12), Some(14)]);
        assert_eq!(x.median(), Some(13));
        assert_eq!(x.p95(), Some(30));
        assert_eq!(x.jitter(), Some(13)); // |10-30| + |30-12| + |12-14| = 40 / 3
        assert_eq!(x.loss_pct(), 20);
        assert_eq!(p(&[None, None]).median(), None);
        assert_eq!(p(&[None, None]).loss_pct(), 100);
    }

    #[test]
    fn ranking_counts_cdn_distance() {
        // Fast resolver that sends you far away loses to a slightly slower one
        // that sends you to a nearby server.
        let mut fast_far = p(&[Some(5); 5]);
        fast_far.cdn = Some(180);
        let mut near = p(&[Some(12); 5]);
        near.cdn = Some(8);
        let mut unmeasured = p(&[Some(3); 5]);
        unmeasured.cdn = None;
        assert_eq!(best(&[Some(fast_far.clone()), Some(near), Some(unmeasured)]), Some(1));
        // Without any CDN data, plain lookup time decides.
        fast_far.cdn = None;
        assert_eq!(best(&[Some(p(&[Some(9); 5])), Some(fast_far)]), Some(1));
    }

    #[test]
    fn ranking_prefers_reliable_then_fast() {
        let lossy_fast = p(&[Some(5), None, None, Some(5), Some(5)]);
        let steady = p(&[Some(9), Some(9), Some(10), Some(9), Some(9)]);
        let slower = p(&[Some(14); 5]);
        let probes = vec![Some(slower), Some(lossy_fast), Some(steady), None];
        assert_eq!(best(&probes), Some(2));
        assert_eq!(best(&[None, Some(p(&[None]))]), None);
    }

    #[test]
    fn random_label_is_dns_safe() {
        let l = random_label();
        assert_eq!(l.len(), 12);
        assert!(l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()));
        assert_ne!(random_label(), random_label());
    }

    /// Hits the network. Run with `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_cloudflare() {
        let r = probe("1.1.1.1");
        println!("{r:?} median={:?} jitter={:?}", r.median(), r.jitter());
        assert!(r.median().is_some());
    }
}
