//! Notification-area icon: quick switching while the window is hidden.
//!
//! Menu and click handlers run on the UI thread from the window's message loop.
//! Network work happens on worker threads, which leave their result in
//! `PENDING`; a thread timer (also on the UI thread) copies it into the menu's
//! status line and the icon tooltip, so feedback shows even when no egui frame
//! is being drawn.

use crate::service;
use dns_switcher::ipc::Request;
use dns_switcher::probe;
use dns_switcher::providers::QUICK;
use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// The app window, so the tray can show it without an egui frame running.
pub static WINDOW: AtomicIsize = AtomicIsize::new(0);
/// Latest message from tray actions, picked up by the timer.
static PENDING: Mutex<Option<String>> = Mutex::new(None);

thread_local! {
    static UI: RefCell<Option<(TrayIcon, MenuItem)>> = const { RefCell::new(None) };
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "DNS Switcher";

/// Creates the icon. Call once, on the UI thread, after the window exists.
/// `adapter` is the adapter tray actions apply to (kept in sync by the app).
pub fn install(adapter: Arc<Mutex<String>>, wake: impl Fn() + Send + Sync + 'static) -> Result<(), String> {
    let status = MenuItem::new("DNS Switcher", false, None);
    let open = MenuItem::new("Open DNS Switcher", true, None);
    let quick: Vec<MenuItem> = QUICK.iter().map(|p| MenuItem::new(format!("Use {}", p.name), true, None)).collect();
    let auto = MenuItem::new("Automatic (your provider's DNS)", true, None);
    let fastest = MenuItem::new("Find fastest and use it", true, None);
    let flush = MenuItem::new("Flush DNS cache", true, None);
    let startup = CheckMenuItem::new("Start with Windows", true, autostart_enabled(), None);
    let quit = MenuItem::new("Quit", true, None);

    let menu = Menu::new();
    let sep = PredefinedMenuItem::separator;
    menu.append_items(&[&status, &sep(), &open, &sep()]).map_err(|e| e.to_string())?;
    for q in &quick {
        menu.append(q).map_err(|e| e.to_string())?;
    }
    menu.append_items(&[&auto, &sep(), &fastest, &flush, &sep(), &startup, &quit])
        .map_err(|e| e.to_string())?;

    let img = image::load_from_memory(include_bytes!("../assets/icon.png")).map_err(|e| e.to_string())?;
    let img = image::imageops::resize(&img.into_rgba8(), 32, 32, image::imageops::FilterType::Lanczos3);
    let icon = Icon::from_rgba(img.into_raw(), 32, 32).map_err(|e| e.to_string())?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("DNS Switcher")
        .with_icon(icon)
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| e.to_string())?;

    let ids = (
        open.id().clone(),
        quick.iter().map(|m| m.id().clone()).collect::<Vec<_>>(),
        auto.id().clone(),
        fastest.id().clone(),
        flush.id().clone(),
        startup.id().clone(),
        quit.id().clone(),
    );
    UI.with(|ui| *ui.borrow_mut() = Some((tray, status)));
    start_timer();

    let wake = Arc::new(wake);
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let (open, quick, auto, fastest, flush, startup, quit) = &ids;
        let id = &e.id;
        let current = adapter.lock().map(|a| a.clone()).unwrap_or_default();
        if id == open {
            show_window();
        } else if let Some(i) = quick.iter().position(|q| q == id) {
            let p = &QUICK[i];
            background(&wake, move || {
                Request::apply(&current, false, p.dns1, p.dns2).and_then(|r| service::perform(&r)).map(|_| {
                    let _ = service::perform(&Request::Flush);
                    format!("Using {}", p.name)
                })
            });
        } else if id == auto {
            background(&wake, move || service::perform(&Request::Restore { adapter: current }).map(|_| "Using your provider's DNS".into()));
        } else if id == fastest {
            set_pending("Testing DNS servers…".into());
            background(&wake, move || {
                let results: Vec<_> = QUICK.iter().map(|p| Some(probe::probe(p.dns1))).collect();
                let i = probe::best(&results).ok_or("No DNS server answered")?;
                let p = &QUICK[i];
                let ms = results[i].as_ref().and_then(|r| r.median()).unwrap_or(0);
                let req = Request::apply(&current, false, p.dns1, p.dns2)?;
                service::perform(&req)?;
                let _ = service::perform(&Request::Flush);
                Ok(format!("Using {} ({ms} ms median)", p.name))
            });
        } else if id == flush {
            background(&wake, || service::perform(&Request::Flush).map(|_| "DNS cache flushed".into()));
        } else if id == startup {
            let on = !autostart_enabled();
            match set_autostart(on) {
                Ok(()) => set_pending(if on { "Starts with Windows" } else { "Won't start with Windows" }.into()),
                Err(e) => set_pending(e),
            }
        } else if id == quit {
            std::process::exit(0);
        }
    }));
    TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            show_window();
        }
    }));
    Ok(())
}

fn background(wake: &Arc<impl Fn() + Send + Sync + 'static>, job: impl FnOnce() -> Result<String, String> + Send + 'static) {
    let wake = wake.clone();
    std::thread::spawn(move || {
        set_pending(job().unwrap_or_else(|e| e));
        wake();
    });
}

pub fn set_pending(msg: String) {
    if let Ok(mut p) = PENDING.lock() {
        *p = Some(msg);
    }
}

/// Polls `PENDING` twice a second on the UI thread.
fn start_timer() {
    use windows_sys::Win32::UI::WindowsAndMessaging::SetTimer;
    unsafe extern "system" fn tick(_: windows_sys::Win32::Foundation::HWND, _: u32, _: usize, _: u32) {
        let Some(msg) = PENDING.lock().ok().and_then(|mut p| p.take()) else { return };
        UI.with(|ui| {
            if let Some((tray, status)) = ui.borrow().as_ref() {
                status.set_text(&msg);
                let _ = tray.set_tooltip(Some(format!("DNS Switcher\n{msg}")));
            }
        });
    }
    unsafe {
        SetTimer(std::ptr::null_mut(), 0, 500, Some(tick));
    }
}

pub fn show_window() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW};
    let hwnd = WINDOW.load(Ordering::Relaxed) as windows_sys::Win32::Foundation::HWND;
    if !hwnd.is_null() {
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }
}

pub fn hide_window() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    let hwnd = WINDOW.load(Ordering::Relaxed) as windows_sys::Win32::Foundation::HWND;
    if !hwnd.is_null() {
        unsafe {
            ShowWindow(hwnd, SW_HIDE);
        }
    }
}

pub fn autostart_enabled() -> bool {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|k| k.get_value::<String, _>(RUN_VALUE))
        .is_ok()
}

/// Adds or removes the per-user "start at sign-in" entry (no admin needed).
pub fn set_autostart(on: bool) -> Result<(), String> {
    let (key, _) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY)
        .map_err(|e| e.to_string())?;
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_value(RUN_VALUE, &format!("\"{}\" --tray", exe.display())).map_err(|e| e.to_string())
    } else {
        match key.delete_value(RUN_VALUE) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}
