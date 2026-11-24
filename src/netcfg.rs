//! Reads an adapter's DNS configuration from the TCP/IP registry keys, which
//! is where `netsh` writes it and where Windows keeps what DHCP handed out:
//!
//! `HKLM\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces\{GUID}`
//! - `NameServer`      static IPv4 servers, comma separated; empty = automatic
//! - `DhcpNameServer`  servers from DHCP (your router/provider), space separated
//! - `DhcpDomain`      local domain from DHCP, e.g. `lan` or `home`
//!
//! `...\Tcpip6\Parameters\Interfaces\{GUID}\NameServer` holds static IPv6 servers.
//!
//! Reading the registry is language-independent, unlike `netsh` output.

use std::net::IpAddr;
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

const V4: &str = r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces";
const V6: &str = r"SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters\Interfaces";

/// `{XXXXXXXX-XXXX-...}` for a network interface index.
pub fn interface_guid(index: u32) -> Option<String> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{ConvertInterfaceIndexToLuid, ConvertInterfaceLuidToGuid};
    use windows_sys::Win32::NetworkManagement::Ndis::NET_LUID_LH;
    unsafe {
        let mut luid: NET_LUID_LH = std::mem::zeroed();
        if ConvertInterfaceIndexToLuid(index, &mut luid) != 0 {
            return None;
        }
        let mut g: windows_sys::core::GUID = std::mem::zeroed();
        if ConvertInterfaceLuidToGuid(&luid, &mut g) != 0 {
            return None;
        }
        Some(format!(
            "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
            g.data1, g.data2, g.data3, g.data4[0], g.data4[1], g.data4[2], g.data4[3], g.data4[4], g.data4[5],
            g.data4[6], g.data4[7]
        ))
    }
}

fn value(base: &str, guid: &str, name: &str) -> Option<String> {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(format!("{base}\\{guid}"))
        .ok()?
        .get_value::<String, _>(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

use dns_switcher::ipc::parse_server_list as parse_servers;

/// Static DNS servers; empty means the adapter uses automatic (DHCP) DNS.
pub fn static_servers(guid: &str, ipv6: bool) -> Vec<IpAddr> {
    value(if ipv6 { V6 } else { V4 }, guid, "NameServer")
        .map(|v| parse_servers(&v, ipv6))
        .unwrap_or_default()
}

/// IPv4 DNS servers handed out by DHCP (router or provider), whether or not
/// they're currently in use.
pub fn dhcp_servers(guid: &str) -> Vec<IpAddr> {
    value(V4, guid, "DhcpNameServer").map(|v| parse_servers(&v, false)).unwrap_or_default()
}

/// Local DNS domain from DHCP or set by hand, e.g. `lan`.
pub fn local_domain(guid: &str) -> Option<String> {
    value(V4, guid, "Domain").or_else(|| value(V4, guid, "DhcpDomain"))
}
