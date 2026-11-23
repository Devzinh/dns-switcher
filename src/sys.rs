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

fn index_of(adapter: &str) -> Result<String, String> {
    adapter_list()
        .into_iter()
        .find(|a| a.name == adapter)
        .map(|a| a.index.to_string())
        .ok_or_else(|| format!("No network adapter named \"{adapter}\"."))
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
            let idx = index_of(adapter)?;
            run("netsh.exe", &["interface", "ipv4", "set", "dnsservers", &idx, "dhcp"])?;
            let _ = run("netsh.exe", &["interface", "ipv6", "set", "dnsservers", &idx, "dhcp"]);
            Ok(format!("Automatic (DHCP) DNS restored on \"{adapter}\"."))
        }
        Request::Apply { adapter, ipv6, dns1, dns2 } => {
            let idx = index_of(adapter)?;
            apply(&idx, *ipv6, *dns1, *dns2)?;
            Ok(format!("DNS successfully changed to {dns1} on \"{adapter}\"."))
        }
    }
}

fn apply(idx: &str, ipv6: bool, dns1: IpAddr, dns2: Option<IpAddr>) -> Result<(), String> {
    let family = if ipv6 { "ipv6" } else { "ipv4" };
    let (d1, d2) = (dns1.to_string(), dns2.map(|d| d.to_string()));
    run("netsh.exe", &["interface", family, "set", "dnsservers", idx, "static", &d1, "primary", "validate=no"])?;
    if let Some(d2) = d2 {
        run("netsh.exe", &["interface", family, "add", "dnsservers", idx, &d2, "index=2", "validate=no"])?;
    }
    Ok(())
}

/// DNS servers in use on an adapter, as a short human string.
pub fn current_dns(adapter: &str) -> String {
    let Ok(idx) = index_of(adapter) else {
        return "Unknown".into();
    };
    let Ok(text) = run("netsh.exe", &["interface", "ipv4", "show", "dnsservers", &idx]) else {
        return "Unknown".into();
    };
    let dhcp = text.to_lowercase().contains("dhcp");
    let mut ips: Vec<String> = Vec::new();
    for tok in text.split(|c: char| c.is_whitespace() || c == ':') {
        if tok.parse::<std::net::Ipv4Addr>().is_ok() && !ips.iter().any(|i| i == tok) {
            ips.push(tok.to_string());
        }
    }
    match (ips.is_empty(), dhcp) {
        (true, true) => "Automatic (DHCP)".into(),
        (true, false) => "Unknown".into(),
        (false, true) => format!("Automatic: {}", ips.join(", ")),
        (false, false) => ips.join(", "),
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
