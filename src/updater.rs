//! Automatic updates, run inside the DNS Switcher service (LocalSystem).
//!
//! The service checks GitHub every six hours when automatic updates are on,
//! or right away when the app asks. A newer release is downloaded into
//! `<install dir>\updates` (writable only by administrators and SYSTEM, so no
//! other user can swap the file after it was verified), checked against the
//! signed hash list (see `dns_switcher::update`), and its installer is run
//! silently. The installer stops and replaces this service, so the new
//! service instance finishes the job: it reopens the tray icon for the user
//! who had it open.
//!
//! Settings live in `HKLM\SOFTWARE\DNS Switcher`:
//! - `AutoUpdate` (DWORD): 1 = install updates automatically. Set by setup.
//! - `UpdateApiUrl`, `UpdatePublicKey` (strings): test overrides only.
//!   Writable only by administrators, who could replace the program anyway.

use dns_switcher::update::{self, Overrides};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};
use winreg::RegKey;

const SETTINGS_KEY: &str = r"SOFTWARE\DNS Switcher";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(2 * 60);
const RELAUNCH_MARKER: &str = "relaunch-tray";

static LAST: Mutex<String> = Mutex::new(String::new());
static BUSY: Mutex<bool> = Mutex::new(false);

fn settings() -> Option<RegKey> {
    RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(SETTINGS_KEY, KEY_READ).ok()
}

pub fn auto_enabled() -> bool {
    settings().and_then(|k| k.get_value::<u32, _>("AutoUpdate").ok()).unwrap_or(0) == 1
}

pub fn set_auto(on: bool) -> Result<String, String> {
    let (key, _) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .create_subkey_with_flags(SETTINGS_KEY, KEY_WRITE)
        .map_err(|e| e.to_string())?;
    key.set_value("AutoUpdate", &u32::from(on)).map_err(|e| e.to_string())?;
    Ok(if on { "Automatic updates are on" } else { "Automatic updates are off" }.into())
}

fn overrides() -> Overrides {
    let k = settings();
    let s = |name: &str| k.as_ref().and_then(|k| k.get_value::<String, _>(name).ok()).filter(|v| !v.is_empty());
    Overrides { api_url: s("UpdateApiUrl"), public_key_hex: s("UpdatePublicKey") }
}

fn updates_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.parent().map(|p| p.join("updates")).unwrap_or_else(|| PathBuf::from("updates"))
}

fn set_last(msg: &str) {
    if let Ok(mut l) = LAST.lock() {
        *l = msg.to_string();
    }
}

pub fn status() -> String {
    let last = LAST.lock().map(|l| l.clone()).unwrap_or_default();
    format!(
        "Version {}. Automatic updates {}.{}",
        env!("CARGO_PKG_VERSION"),
        if auto_enabled() { "on" } else { "off" },
        if last.is_empty() { String::new() } else { format!(" {last}") }
    )
}

/// Checks GitHub and, if a newer signed release exists, installs it.
pub fn check_and_install() -> Result<String, String> {
    {
        let mut busy = BUSY.lock().map_err(|_| "busy")?;
        if *busy {
            return Ok("An update check is already running".into());
        }
        *busy = true;
    }
    let result = (|| {
        let o = overrides();
        let Some(release) = update::latest(&o)? else {
            return Ok(format!("Up to date ({})", env!("CARGO_PKG_VERSION")));
        };
        if release.version <= update::current_version() {
            return Ok(format!("Up to date ({})", env!("CARGO_PKG_VERSION")));
        }
        let installer = update::download_verified(&release, &updates_dir(), &o)?;
        install(&installer)?;
        Ok(format!("Installing version {}", update::fmt_version(release.version)))
    })();
    if let Ok(mut b) = BUSY.lock() {
        *b = false;
    }
    set_last(&match &result {
        Ok(m) => format!("Last check: {m}."),
        Err(e) => format!("Last check failed: {e}."),
    });
    result
}

/// Starts the verified installer. It stops this service, so remember whether
/// the tray icon was open to bring it back afterwards.
fn install(installer: &std::path::Path) -> Result<(), String> {
    let dir = updates_dir();
    if tray_running() {
        let _ = std::fs::write(dir.join(RELAUNCH_MARKER), b"");
    }
    let log = dir.join("install.log");
    std::process::Command::new(installer)
        .args(["/SP-", "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/FROMSERVICE"])
        .arg(format!("/LOG={}", log.display()))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start the installer: {e}"))
}

/// Background loop: first check shortly after start, then every six hours.
pub fn start_background() {
    std::thread::spawn(|| {
        after_update();
        std::thread::sleep(FIRST_CHECK_AFTER);
        loop {
            remove_old_installers();
            if auto_enabled() {
                let _ = check_and_install();
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

/// Runs once when the service starts: cleans up and, if an update just
/// replaced the app while its tray icon was open, opens it again.
fn after_update() {
    let dir = updates_dir();
    let marker = dir.join(RELAUNCH_MARKER);
    let relaunch = marker.exists();
    let _ = std::fs::remove_file(&marker);
    if relaunch {
        // Give the installer a moment to finish before starting the app.
        std::thread::sleep(Duration::from_secs(5));
        if let Err(e) = launch_tray_for_console_user() {
            set_last(&format!("Updated, but could not reopen the tray icon: {e}."));
        } else {
            set_last(&format!("Updated to {}.", env!("CARGO_PKG_VERSION")));
        }
    }
}

/// Deletes downloaded installers. The one that just ran may still be locked
/// right after an update, so this also runs before every later check.
fn remove_old_installers() {
    if let Ok(entries) = std::fs::read_dir(updates_dir()) {
        for e in entries.flatten() {
            if e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// True if any other dns_switcher.exe (the app) is running.
fn tray_running() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let me = std::process::id();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if name.eq_ignore_ascii_case("dns_switcher.exe") && entry.th32ProcessID != me {
                found = true;
                break;
            }
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
        found
    }
}

/// Starts `dns_switcher.exe --tray` as the user signed in at the console.
fn launch_tray_for_console_user() -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows_sys::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
    use windows_sys::Win32::System::RemoteDesktop::{WTSGetActiveConsoleSessionId, WTSQueryUserToken};
    use windows_sys::Win32::System::Threading::{
        CreateProcessAsUserW, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
    };
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd: Vec<u16> = format!("\"{}\" --tray\0", exe.display()).encode_utf16().collect();
    let mut desktop: Vec<u16> = "winsta0\\default\0".encode_utf16().collect();
    unsafe {
        let session = WTSGetActiveConsoleSessionId();
        if session == u32::MAX {
            return Err("nobody is signed in".into());
        }
        let mut token: HANDLE = std::ptr::null_mut();
        if WTSQueryUserToken(session, &mut token) == 0 {
            return Err(format!("no user token (error {})", GetLastError()));
        }
        let mut env = std::ptr::null_mut();
        let have_env = CreateEnvironmentBlock(&mut env, token, 0) != 0;
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.lpDesktop = desktop.as_mut_ptr();
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessAsUserW(
            token,
            std::ptr::null(),
            cmd.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_UNICODE_ENVIRONMENT,
            if have_env { env } else { std::ptr::null() },
            std::ptr::null(),
            &si,
            &mut pi,
        );
        let err = GetLastError();
        if have_env {
            DestroyEnvironmentBlock(env);
        }
        CloseHandle(token);
        if ok == 0 {
            return Err(format!("CreateProcessAsUser failed (error {err})"));
        }
        CloseHandle(pi.hProcess);
        CloseHandle(pi.hThread);
        Ok(())
    }
}
