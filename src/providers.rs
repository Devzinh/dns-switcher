// Curated public DNS resolvers, grouped by purpose (Default / Secure / Family Safe).
// Each entry: display name, primary, secondary (empty = none).

pub struct Provider {
    pub name: &'static str,
    pub dns1: &'static str,
    pub dns2: &'static str,
}

pub struct Group {
    pub title: &'static str,
    pub ipv4: &'static [Provider],
    pub ipv6: &'static [Provider],
}

macro_rules! p {
    ($n:expr, $a:expr, $b:expr) => {
        Provider { name: $n, dns1: $a, dns2: $b }
    };
}

pub const DEFAULT_V4: &[Provider] = &[
    p!("Google Public DNS", "8.8.8.8", "8.8.4.4"),
    p!("Cloudflare", "1.1.1.1", "1.0.0.1"),
    p!("OpenDNS", "208.67.222.222", "208.67.220.220"),
    p!("OpenDNS - 2", "208.67.222.220", "208.67.220.222"),
    p!("Quad9 Security", "9.9.9.9", "149.112.112.112"),
    p!("Quad9 No Security", "9.9.9.10", "149.112.112.10"),
    p!("AdGuard", "94.140.14.14", "94.140.15.15"),
    p!("Yandex", "77.88.8.1", "77.88.8.8"),
    p!("NextDNS", "45.90.28.230", "45.90.30.230"),
    p!("DNS4EU", "86.54.11.100", "86.54.11.200"),
    p!("Comodo Secure", "8.26.56.26", "8.20.247.20"),
    p!("Level 3 - A", "209.244.0.3", "209.244.0.4"),
    p!("Level 3 - B", "4.2.2.1", "4.2.2.2"),
    p!("Verisign Public DNS", "64.6.64.6", "64.6.65.6"),
    p!("Norton ConnectSafe Basic", "199.85.126.10", "199.85.127.10"),
    p!("Neustar 1", "156.154.70.1", "156.154.71.1"),
    p!("Neustar 2", "156.154.70.5", "156.154.71.5"),
    p!("Hurricane Electric", "74.82.42.42", ""),
    p!("Safe DNS", "195.46.39.39", "195.46.39.40"),
    p!("UltraDNS", "204.69.234.1", "204.74.101.1"),
    p!("Dyn", "216.146.35.35", "216.146.36.36"),
];

pub const SECURE_V4: &[Provider] = &[
    p!("Cloudflare Malware Blocking", "1.1.1.2", "1.0.0.2"),
    p!("Norton ConnectSafe Secure", "199.85.126.20", "199.85.127.20"),
    p!("Yandex Safe", "77.88.8.88", "77.88.8.2"),
    p!("Comodo Secure", "8.26.56.26", "8.20.247.20"),
    p!("CleanBrowsing Secure", "185.228.168.9", "185.228.169.9"),
    p!("Neustar Threat Protection", "156.154.70.2", "156.154.71.2"),
    p!("DNS4EU Protective", "86.54.11.13", "86.54.11.213"),
];

pub const FAMILY_V4: &[Provider] = &[
    p!("Cloudflare Malware + Adult", "1.1.1.3", "1.0.0.3"),
    p!("OpenDNS Family", "208.67.222.123", "208.67.220.123"),
    p!("CleanBrowsing Family", "185.228.168.168", "185.228.169.168"),
    p!("CleanBrowsing Adult Filter", "185.228.168.10", "185.228.169.11"),
    p!("Norton ConnectSafe Family", "199.85.126.30", "199.85.127.30"),
    p!("Yandex Family", "77.88.8.7", "77.88.8.3"),
    p!("AdGuard Family", "94.140.14.15", "94.140.15.16"),
    p!("Neustar Family Secure", "156.154.70.3", "156.154.71.3"),
    p!("DNS4EU Child", "86.54.11.11", "86.54.11.211"),
];

pub const DEFAULT_V6: &[Provider] = &[
    p!("Google Public DNS", "2001:4860:4860::8888", "2001:4860:4860::8844"),
    p!("Cloudflare", "2606:4700:4700::1111", "2606:4700:4700::1001"),
    p!("OpenDNS Sandbox", "2620:0:ccc::2", "2620:0:ccd::2"),
    p!("Quad9 Security", "2620:fe::fe", "2620:fe::9"),
    p!("Quad9 No Security", "2620:fe::10", "2620:fe::fe:10"),
    p!("AdGuard", "2a10:50c0::ad1:ff", "2a10:50c0::ad2:ff"),
    p!("Yandex DNS", "2a02:6b8::feed:0ff", "2a02:6b8:0:1::feed:0ff"),
    p!("NextDNS", "2a07:a8c0::46:1af1", "2a07:a8c1::46:1af1"),
    p!("DNS4EU", "2a13:1001::86:54:11:100", "2a13:1001::86:54:11:200"),
];

pub const SECURE_V6: &[Provider] = &[
    p!("Cloudflare Malware Blocking", "2606:4700:4700::1112", "2606:4700:4700::1002"),
    p!("Yandex Safe", "2a02:6b8::feed:bad", "2a02:6b8:0:1::feed:bad"),
    p!("CleanBrowsing Secure", "2a0d:2a00:1::2", "2a0d:2a00:2::2"),
    p!("Neustar Threat Protection", "2610:a1:1018::2", "2610:a1:1019::2"),
    p!("DNS4EU Protective", "2a13:1001::86:54:11:13", "2a13:1001::86:54:11:213"),
];

pub const FAMILY_V6: &[Provider] = &[
    p!("Cloudflare Malware + Adult", "2606:4700:4700::1113", "2606:4700:4700::1003"),
    p!("CleanBrowsing Family", "2a0d:2a00:1::", "2a0d:2a00:2::"),
    p!("CleanBrowsing Adult Filter", "2a0d:2a00:1::1", "2a0d:2a00:2::1"),
    p!("Yandex Family", "2a02:6b8::feed:a11", "2a02:6b8:0:1::feed:a11"),
    p!("AdGuard Family", "2a10:50c0::bad1:ff", "2a10:50c0::bad2:ff"),
    p!("Neustar Family Secure", "2610:a1:1018::3", "2610:a1:1019::3"),
    p!("DNS4EU Child", "2a13:1001::86:54:11:11", "2a13:1001::86:54:11:211"),
];

pub const GROUPS: &[Group] = &[
    Group { title: "Default", ipv4: DEFAULT_V4, ipv6: DEFAULT_V6 },
    Group { title: "Secure", ipv4: SECURE_V4, ipv6: SECURE_V6 },
    Group { title: "Family Safe", ipv4: FAMILY_V4, ipv6: FAMILY_V6 },
];

// Beginner-facing favourites (subset, IPv4).
pub const QUICK: &[Provider] = &[
    p!("Cloudflare — fast & private", "1.1.1.1", "1.0.0.1"),
    p!("Google — reliable", "8.8.8.8", "8.8.4.4"),
    p!("Quad9 — blocks malware", "9.9.9.9", "149.112.112.112"),
    p!("AdGuard — blocks ads", "94.140.14.14", "94.140.15.15"),
    p!("OpenDNS — family friendly", "208.67.222.123", "208.67.220.123"),
];
