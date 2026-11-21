// System glue: adapters, applying DNS, flush, and a UDP DNS resolve-time probe.
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

#[cfg(windows)]
const NO_WINDOW: u32 = 0x0800_0000; // CREATE_NO_WINDOW

#[cfg(windows)]
fn run(args: &[&str]) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    let out = Command::new("cmd")
        .args(["/C"])
        .args(args)
        .creation_flags(NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(s)
    } else {
        Err(if s.trim().is_empty() {
            format!("command failed (exit {:?})", out.status.code())
        } else {
            s
        })
    }
}

#[cfg(not(windows))]
fn run(_args: &[&str]) -> Result<String, String> {
    Err("This action needs Windows.".into())
}

/// List connected network adapter names (locale-independent parse).
pub fn adapters() -> Vec<String> {
    #[cfg(windows)]
    {
        if let Ok(text) = run(&["netsh", "interface", "ipv4", "show", "interfaces"]) {
            let mut names = Vec::new();
            for line in text.lines() {
                let cols: Vec<&str> = line.split_whitespace().collect();
                // Idx Met MTU State Name...
                if cols.len() >= 5 && cols[0].parse::<u32>().is_ok() {
                    let name = cols[4..].join(" ");
                    let low = name.to_lowercase();
                    if low.contains("loopback") {
                        continue;
                    }
                    names.push(name);
                }
            }
            if !names.is_empty() {
                return names;
            }
        }
        vec!["Ethernet".into(), "Wi-Fi".into()]
    }
    #[cfg(not(windows))]
    {
        vec!["eth0".into()]
    }
}

pub fn apply_dns(adapter: &str, ipv6: bool, dns1: &str, dns2: &str) -> Result<(), String> {
    let family = if ipv6 { "ipv6" } else { "ipv4" };
    let name = format!("name={}", adapter);
    run(&[
        "netsh", "interface", family, "set", "dnsservers", &name, "static", dns1, "primary",
    ])?;
    if !dns2.trim().is_empty() {
        run(&[
            "netsh", "interface", family, "add", "dnsservers", &name, dns2, "index=2",
        ])?;
    }
    Ok(())
}

pub fn restore_dhcp(adapter: &str) -> Result<(), String> {
    let name = format!("name={}", adapter);
    run(&["netsh", "interface", "ipv4", "set", "dnsservers", &name, "dhcp"])?;
    let _ = run(&["netsh", "interface", "ipv6", "set", "dnsservers", &name, "dhcp"]);
    Ok(())
}

pub fn flush_dns() -> Result<(), String> {
    run(&["ipconfig", "/flushdns"]).map(|_| ())
}

/// Currently applied DNS servers for an adapter, as a short human string.
pub fn current_dns(adapter: &str) -> String {
    #[cfg(windows)]
    {
        let name = format!("name={}", adapter);
        if let Ok(text) = run(&["netsh", "interface", "ipv4", "show", "dnsservers", &name]) {
            let low = text.to_lowercase();
            let mut ips: Vec<String> = Vec::new();
            for tok in text.split(|c: char| c.is_whitespace() || c == ':') {
                let t = tok.trim();
                if is_ipv4(t) && !ips.contains(&t.to_string()) {
                    ips.push(t.to_string());
                }
            }
            if ips.is_empty() {
                if low.contains("dhcp") {
                    return "Automatic (DHCP)".into();
                }
                return "Unknown".into();
            }
            // DHCP-assigned servers still list IPs; flag automatic when the word appears.
            let prefix = if low.contains("dhcp") { "Auto: " } else { "" };
            return format!("{}{}", prefix, ips.join(" · "));
        }
        "Unknown".into()
    }
    #[cfg(not(windows))]
    {
        let _ = adapter;
        "Unknown".into()
    }
}

fn is_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.parse::<u8>().is_ok())
}

/// One UDP DNS A-query to `server`, measure round-trip ms. None on timeout/error.
pub fn resolve_ms(server: &str, domain: &str, timeout_ms: u64) -> Option<u32> {
    let ip: IpAddr = server.trim().parse().ok()?;
    let bind = if ip.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
    let sock = UdpSocket::bind(bind).ok()?;
    sock.set_read_timeout(Some(Duration::from_millis(timeout_ms))).ok()?;
    let target = SocketAddr::new(ip, 53);
    let query = build_query(domain);
    let start = Instant::now();
    sock.send_to(&query, target).ok()?;
    let mut buf = [0u8; 512];
    match sock.recv_from(&mut buf) {
        Ok((n, _)) if n >= 12 => Some(start.elapsed().as_millis() as u32),
        _ => None,
    }
}

fn build_query(domain: &str) -> Vec<u8> {
    let mut q = Vec::with_capacity(32);
    q.extend_from_slice(&[0x12, 0x34]); // id
    q.extend_from_slice(&[0x01, 0x00]); // RD
    q.extend_from_slice(&[0x00, 0x01]); // qdcount
    q.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    for label in domain.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&[0x00, 0x01]); // A
    q.extend_from_slice(&[0x00, 0x01]); // IN
    q
}
