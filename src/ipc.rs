use std::net::IpAddr;

pub const PIPE_NAME: &str = r"\\.\pipe\DnsSwitcherService";
pub const SERVICE_NAME: &str = "DnsSwitcher";
pub const MAX_LINE: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Ping,
    Flush,
    Restore { adapter: String },
    UpdateStatus,
    UpdateNow,
    AutoUpdate(bool),
    Apply { adapter: String, ipv6: bool, dns1: IpAddr, dns2: Option<IpAddr> },
}

impl Request {
    pub fn apply(adapter: &str, ipv6: bool, dns1: &str, dns2: &str) -> Result<Request, String> {
        let adapter = check_adapter(adapter)?;
        let dns1 = parse_ip(dns1, ipv6)?;
        let dns2 = match dns2.trim() {
            "" => None,
            s => Some(parse_ip(s, ipv6)?),
        }
        .filter(|d| *d != dns1);
        Ok(Request::Apply { adapter, ipv6, dns1, dns2 })
    }

    pub fn encode(&self) -> String {
        match self {
            Request::Ping => "PING".into(),
            Request::Flush => "FLUSH".into(),
            Request::UpdateStatus => "UPDATE_STATUS".into(),
            Request::UpdateNow => "UPDATE_NOW".into(),
            Request::AutoUpdate(on) => format!("AUTOUPDATE\t{}", u8::from(*on)),
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
            ["UPDATE_STATUS"] => Ok(Request::UpdateStatus),
            ["UPDATE_NOW"] => Ok(Request::UpdateNow),
            ["AUTOUPDATE", "1"] => Ok(Request::AutoUpdate(true)),
            ["AUTOUPDATE", "0"] => Ok(Request::AutoUpdate(false)),
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

pub fn parse_server_list(list: &str, ipv6: bool) -> Vec<IpAddr> {
    list.split([',', ' ', ';'])
        .filter_map(|s| s.trim().parse::<IpAddr>().ok())
        .filter(|ip| ip.is_ipv6() == ipv6)
        .collect()
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
            Request::UpdateStatus,
            Request::UpdateNow,
            Request::AutoUpdate(true),
            Request::AutoUpdate(false),
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
        assert!(Request::apply("Ethernet", false, "2606:4700::1111", "").is_err());
        assert!(Request::apply("Ethernet", true, "1.1.1.1", "").is_err());
        assert!(Request::apply("Ethernet", false, "0.0.0.0", "").is_err());
        assert!(Request::apply("Ethernet", false, "224.0.0.1", "").is_err());
        assert!(Request::apply("Ethernet", false, "1.1.1.1", "nope").is_err());
    }

    #[test]
    fn duplicate_secondary_is_dropped() {
        let r = Request::apply("Ethernet", false, "1.1.1.1", "1.1.1.1").unwrap();
        assert_eq!(r, Request::Apply { adapter: "Ethernet".into(), ipv6: false, dns1: "1.1.1.1".parse().unwrap(), dns2: None });
    }

    #[test]
    fn rejects_bad_requests() {
        assert!(Request::decode("APPLY\tEthernet\t5\t1.1.1.1\t").is_err());
        assert!(Request::decode("RESTORE\tbad\"name").is_err());
        assert!(Request::decode("RESTORE\t").is_err());
        assert!(Request::decode("RUN\tcmd.exe").is_err());
        assert!(Request::decode("AUTOUPDATE\tyes").is_err());
        assert!(Request::decode(&"A".repeat(MAX_LINE + 1)).is_err());
    }

    #[test]
    fn server_lists() {
        assert_eq!(parse_server_list("1.1.1.1,1.0.0.1", false).len(), 2);
        assert_eq!(parse_server_list("192.168.1.1 8.8.8.8", false).len(), 2);
        assert_eq!(parse_server_list("2606:4700:4700::1111,2606:4700:4700::1001", true).len(), 2);
        assert!(parse_server_list("2606:4700:4700::1111", false).is_empty());
        assert!(parse_server_list("", false).is_empty());
        assert!(parse_server_list("garbage, ,", false).is_empty());
    }

    #[test]
    fn replies() {
        assert_eq!(decode_reply(&encode_reply(&Ok("done\nnow".into()))), Ok("done now".into()));
        assert_eq!(decode_reply(&encode_reply(&Err("no".into()))), Err("no".into()));
        assert!(decode_reply("garbage").is_err());
    }
}
