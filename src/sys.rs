//! System glue: adapters, applying DNS, restoring DHCP and flushing the cache.
//!
//! Commands are started directly from System32 by absolute path, never through
//! a shell, and adapters are addressed by interface index. This code also runs
//! inside the LocalSystem service, so no request field may reach a command line
//! other than as a validated IP address or a number.

use dns_switcher::ipc::Request;
use std::net::IpAddr;

#[cfg(windows)]
const NO_WINDOW: u32 = 0x0800_0000; // CREATE_NO_WINDOW

#[cfg(windows)]
fn system32(exe: &str) -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    std::path::Path::new(&root).join("System32").join(exe)
}

#[cfg(windows)]
fn run(exe: &str, args: &[&str]) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new(system32(exe))
        .args(args)
        .creation_flags(NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(s)
    } else if s.trim().is_empty() {
        Err(format!("{exe} failed (exit {:?})", out.status.code()))
    } else {
        Err(s)
    }
}

#[cfg(not(windows))]
fn run(_exe: &str, _args: &[&str]) -> Result<String, String> {
    Err("This action needs Windows.".into())
}

pub struct Adapter {
    pub index: u32,
    pub name: String,
}

/// Network interfaces except loopback, from `netsh interface ipv4 show interfaces`.
/// Columns are Idx, Met, MTU, State, Name; parsing by position keeps it
/// independent of the Windows display language.
pub fn adapter_list() -> Vec<Adapter> {
    let Ok(text) = run("netsh.exe", &["interface", "ipv4", "show", "interfaces"]) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            let index = cols.first()?.parse::<u32>().ok()?;
            if cols.len() < 5 {
                return None;
            }
            let name = cols[4..].join(" ");
            (!name.to_lowercase().contains("loopback")).then_some(Adapter { index, name })
        })
        .collect()
}

pub fn adapters() -> Vec<String> {
    let names: Vec<String> = adapter_list().into_iter().map(|a| a.name).collect();
    if names.is_empty() {
        vec!["Ethernet".into(), "Wi-Fi".into()]
    } else {
        names
    }
}

fn index_of(adapter: &str) -> Result<u32, String> {
    adapter_list()
        .into_iter()
        .find(|a| a.name == adapter)
        .map(|a| a.index)
        .ok_or_else(|| format!("No network adapter named \"{adapter}\"."))
}

/// DNS servers your router or provider handed out by DHCP for this adapter,
/// whether or not they're in use right now.
pub fn provider_dns(adapter: &str) -> Vec<IpAddr> {
    index_of(adapter)
        .ok()
        .and_then(crate::netcfg::interface_guid)
        .map(|g| crate::netcfg::dhcp_servers(&g))
        .unwrap_or_default()
}

/// Runs a request with the current process's rights. Used by the service, and
/// by the app itself when it already runs as administrator.
pub fn execute(req: &Request) -> Result<String, String> {
    match req {
        Request::Ping => Ok("pong".into()),
        Request::UpdateStatus | Request::UpdateNow | Request::AutoUpdate(_) => {
            Err("Updates are handled by the DNS Switcher service; install DNS Switcher with its setup program.".into())
        }
        Request::Flush => {
            run("ipconfig.exe", &["/flushdns"])?;
            Ok("Successfully flushed the DNS resolver cache.".into())
        }
        Request::Restore { adapter } => {
            let idx = index_of(adapter)?.to_string();
            run("netsh.exe", &["interface", "ipv4", "set", "dnsservers", &idx, "dhcp"])?;
            let _ = run("netsh.exe", &["interface", "ipv6", "set", "dnsservers", &idx, "dhcp"]);
            Ok(format!("Automatic (DHCP) DNS restored on \"{adapter}\"."))
        }
        Request::Apply { adapter, ipv6, dns1, dns2 } => {
            let idx = index_of(adapter)?;
            let guid = crate::netcfg::interface_guid(idx);
            let mut want = vec![*dns1];
            want.extend(*dns2);
            apply_all_or_nothing(idx, guid.as_deref(), *ipv6, &want)?;
            let mut msg = format!("DNS successfully changed to {dns1} on \"{adapter}\".");
            if let Some(domain) = guid.as_deref().and_then(crate::netcfg::local_domain) {
                msg.push_str(&format!(
                    " Names on your local network (*.{domain}) may stop resolving; click Automatic to undo."
                ));
            }
            Ok(msg)
        }
    }
}

/// Applies the servers, reads the result back, and on any failure puts the
/// adapter back exactly as it was, so it's never left half-configured.
fn apply_all_or_nothing(idx: u32, guid: Option<&str>, ipv6: bool, want: &[IpAddr]) -> Result<(), String> {
    let before = guid.map(|g| crate::netcfg::static_servers(g, ipv6));
    let result = set_servers(idx, ipv6, want).and_then(|()| match guid {
        Some(g) => {
            let now = crate::netcfg::static_servers(g, ipv6);
            if now == want {
                Ok(())
            } else {
                Err(format!("Windows reports {} instead", list(&now)))
            }
        }
        None => Ok(()),
    });
    let Err(e) = result else { return Ok(()) };
    let first = e.lines().find(|l| !l.trim().is_empty()).unwrap_or(&e).trim().trim_end_matches('.').to_string();
    match before {
        None => Err(format!("Could not change DNS: {first}. The previous settings could not be read, so check them in Windows settings.")),
        Some(prev) => match set_servers(idx, ipv6, &prev) {
            Ok(()) => Err(format!("Could not change DNS: {first}. Your previous settings ({}) were restored.", list(&prev))),
            Err(e2) => Err(format!("Could not change DNS: {first}. Restoring the previous settings also failed: {e2}")),
        },
    }
}

fn list(servers: &[IpAddr]) -> String {
    if servers.is_empty() {
        "automatic".into()
    } else {
        servers.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ")
    }
}

/// Sets the adapter's static servers in order, or automatic (DHCP) when empty.
fn set_servers(idx: u32, ipv6: bool, servers: &[IpAddr]) -> Result<(), String> {
    let family = if ipv6 { "ipv6" } else { "ipv4" };
    let idx = idx.to_string();
    let Some((first, rest)) = servers.split_first() else {
        return run("netsh.exe", &["interface", family, "set", "dnsservers", &idx, "dhcp"]).map(|_| ());
    };
    let first = first.to_string();
    run("netsh.exe", &["interface", family, "set", "dnsservers", &idx, "static", &first, "primary", "validate=no"])?;
    for (i, s) in rest.iter().enumerate() {
        let (s, pos) = (s.to_string(), format!("index={}", i + 2));
        run("netsh.exe", &["interface", family, "add", "dnsservers", &idx, &s, &pos, "validate=no"])?;
    }
    Ok(())
}

/// DNS servers in use on an adapter, as a short human string.
pub fn current_dns(adapter: &str) -> String {
    let Some(guid) = index_of(adapter).ok().and_then(crate::netcfg::interface_guid) else {
        return "Unknown".into();
    };
    let fixed = crate::netcfg::static_servers(&guid, false);
    if !fixed.is_empty() {
        return list(&fixed);
    }
    match crate::netcfg::dhcp_servers(&guid) {
        d if d.is_empty() => "Automatic (DHCP)".into(),
        d => format!("Automatic: {}", list(&d)),
    }
}

/// True when this process has an elevated (administrator) token.
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elev = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elev as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && elev.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}
