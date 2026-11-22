//! Wire protocol between the app and the DNS Switcher service.
//!
//! The service runs as LocalSystem and is the only part that changes network
//! settings. Requests travel over a local named pipe as one line of
//! tab-separated fields; replies are one line starting with `OK` or `ERR`.
//!
//! Everything a request carries is re-validated by the service: addresses must
//! parse as IP addresses of the requested family, and the adapter must exist
//! (the service maps it to an interface index itself). No field is ever passed
//! to a shell.

use std::net::IpAddr;

/// Named pipe the service listens on.
pub const PIPE_NAME: &str = r"\\.\pipe\DnsSwitcherService";
/// Windows service name (as in `sc query DnsSwitcher`).
pub const SERVICE_NAME: &str = "DnsSwitcher";
/// Longest request line the service will read.
pub const MAX_LINE: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Ping,
    Flush,
    Restore { adapter: String },
    Apply { adapter: String, ipv6: bool, dns1: IpAddr, dns2: Option<IpAddr> },
}

impl Request {
    /// Builds an `Apply`, checking both addresses belong to the chosen family.
    pub fn apply(adapter: &str, ipv6: bool, dns1: &str, dns2: &str) -> Result<Request, String> {
        let adapter = check_adapter(adapter)?;
        let dns1 = parse_ip(dns1, ipv6)?;
        let dns2 = match dns2.trim() {
            "" => None,
            s => Some(parse_ip(s, ipv6)?),
        };
        Ok(Request::Apply { adapter, ipv6, dns1, dns2 })
    }

    pub fn encode(&self) -> String {
        match self {
            Request::Ping => "PING".into(),
            Request::Flush => "FLUSH".into(),
            Request::Restore { adapter } => format!("RESTORE\t{adapter}"),
            Request::Apply { adapter, ipv6, dns1, dns2 } => format!(
                "APPLY\t{adapter}\t{}\t{dns1}\t{}",
                if *ipv6 { 6 } else { 4 },
                dns2.map(|d| d.to_string()).unwrap_or_default()
            ),
        }
    }

    pub fn decode(line: &str) -> Result<Request, String> {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.len() > MAX_LINE {
            return Err("request too long".into());
        }
        let f: Vec<&str> = line.split('\t').collect();
        match f.as_slice() {
            ["PING"] => Ok(Request::Ping),
            ["FLUSH"] => Ok(Request::Flush),
            ["RESTORE", adapter] => Ok(Request::Restore { adapter: check_adapter(adapter)? }),
            ["APPLY", adapter, fam, dns1, dns2] => {
                let ipv6 = match *fam {
                    "4" => false,
                    "6" => true,
                    _ => return Err("bad address family".into()),
                };
                Request::apply(adapter, ipv6, dns1, dns2)
            }
            _ => Err("unknown request".into()),
        }
    }
}

/// Reply line for a result.
pub fn encode_reply(r: &Result<String, String>) -> String {
    let clean = |s: &str| s.replace(['\r', '\n', '\t'], " ");
    match r {
        Ok(m) => format!("OK\t{}", clean(m)),
        Err(e) => format!("ERR\t{}", clean(e)),
    }
}

pub fn decode_reply(line: &str) -> Result<String, String> {
    let line = line.trim_end_matches(['\r', '\n']);
    match line.split_once('\t') {
        Some(("OK", m)) => Ok(m.to_string()),
        Some(("ERR", e)) => Err(e.to_string()),
        _ if line == "OK" => Ok(String::new()),
        _ => Err(format!("unexpected reply from service: {line:?}")),
    }
}

fn parse_ip(s: &str, ipv6: bool) -> Result<IpAddr, String> {
    let ip: IpAddr = s.trim().parse().map_err(|_| format!("{s:?} is not an IP address"))?;
    if ip.is_ipv6() != ipv6 {
        return Err(format!("{ip} is not an IPv{} address", if ipv6 { 6 } else { 4 }));
    }
    if ip.is_unspecified() || ip.is_multicast() {
        return Err(format!("{ip} can't be a DNS server"));
    }
    Ok(ip)
}

/// Adapter names are only compared against the system's own list, but reject
/// anything that could never be one before it gets that far.
fn check_adapter(name: &str) -> Result<String, String> {
    let ok = !name.is_empty()
        && name.len() <= 256
        && !name.chars().any(|c| c.is_control() || matches!(c, '"' | '\t'));
    if ok {
        Ok(name.to_string())
    } else {
        Err("invalid adapter name".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let reqs = [
            Request::Ping,
            Request::Flush,
            Request::Restore { adapter: "Wi-Fi 2".into() },
            Request::apply("Ethernet", false, "1.1.1.1", "1.0.0.1").unwrap(),
            Request::apply("Ethernet", true, "2606:4700:4700::1111", "").unwrap(),
        ];
        for r in reqs {
            assert_eq!(Request::decode(&r.encode()).unwrap(), r);
        }
    }

    #[test]
    fn rejects_bad_addresses() {
        assert!(Request::apply("Ethernet", false, "1.1.1.1 & calc", "").is_err());
        assert!(Request::apply("Ethernet", false, "2606:4700::1111", "").is_err()); // wrong family
        assert!(Request::apply("Ethernet", true, "1.1.1.1", "").is_err());
        assert!(Request::apply("Ethernet", false, "0.0.0.0", "").is_err());
        assert!(Request::apply("Ethernet", false, "224.0.0.1", "").is_err());
        assert!(Request::apply("Ethernet", false, "1.1.1.1", "nope").is_err());
    }

    #[test]
    fn rejects_bad_requests() {
        assert!(Request::decode("APPLY\tEthernet\t5\t1.1.1.1\t").is_err());
        assert!(Request::decode("RESTORE\tbad\"name").is_err());
        assert!(Request::decode("RESTORE\t").is_err());
        assert!(Request::decode("RUN\tcmd.exe").is_err());
        assert!(Request::decode(&"A".repeat(MAX_LINE + 1)).is_err());
    }

    #[test]
    fn replies() {
        assert_eq!(decode_reply(&encode_reply(&Ok("done\nnow".into()))), Ok("done now".into()));
        assert_eq!(decode_reply(&encode_reply(&Err("no".into()))), Err("no".into()));
        assert!(decode_reply("garbage").is_err());
    }
}
