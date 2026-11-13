//! Tray icon (Windows): minimize and close hide the window in the tray, one click on the icon
//! shows or hides it again, and its menu ends the app. Launching a second copy brings the open
//! window forward instead.

use eframe::egui;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicIsize, Ordering};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SW_HIDE,
    SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow,
};

const TITLE: &str = "AgentUsage";

static HWND_: AtomicIsize = AtomicIsize::new(0);
static CTX: OnceLock<egui::Context> = OnceLock::new();

// Keeps the icon alive; taken (and so removed from the tray) just before the app ends.
thread_local! {
    static ICON: std::cell::RefCell<Option<TrayIcon>> = const { std::cell::RefCell::new(None) };
}

/// Create the tray icon for the window behind `cc`. Runs on the main thread.
pub fn install(cc: &eframe::CreationContext<'_>, end_label: &str) -> Option<MenuItem> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let RawWindowHandle::Win32(h) = cc.window_handle().ok()?.as_raw() else {
        return None;
    };
    HWND_.store(h.hwnd.get(), Ordering::Relaxed);
    let _ = CTX.set(cc.egui_ctx.clone());

    let end = MenuItem::new(end_label, true, None);
    let menu = Menu::new();
    menu.append(&end).ok()?;
    let rgba = image::load_from_memory(crate::icons::APP_ICON)
        .ok()?
        .resize(32, 32, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let icon = tray_icon::Icon::from_rgba(rgba.into_raw(), 32, 32).ok()?;
    let tray = TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip(TITLE)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .ok()?;
    ICON.with(|i| *i.borrow_mut() = Some(tray));

    TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = e
        {
            toggle();
        }
    }));
    let end_id = end.id().clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id == end_id {
            quit();
        }
    }));
    Some(end)
}

fn hwnd() -> Option<HWND> {
    let h = HWND_.load(Ordering::Relaxed);
    (h != 0).then_some(h as HWND)
}

/// Hide the window in the tray. False when there is no tray icon (the window stays as it is).
pub fn hide() -> bool {
    let Some(h) = hwnd() else { return false };
    unsafe { ShowWindow(h, SW_HIDE) };
    true
}

fn show(h: HWND) {
    unsafe {
        ShowWindow(h, SW_SHOW);
        if IsIconic(h) != 0 {
            ShowWindow(h, SW_RESTORE);
        }
        SetForegroundWindow(h);
    }
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

fn toggle() {
    let Some(h) = hwnd() else { return };
    let shown = unsafe { IsWindowVisible(h) != 0 && IsIconic(h) == 0 };
    if shown {
        hide();
    } else {
        show(h)
    }
}

/// End the app: remove the tray icon first, so no ghost icon stays behind.
pub fn quit() -> ! {
    ICON.with(|i| i.borrow_mut().take());
    std::process::exit(0)
}

/// If another copy of the app already has its window, bring it forward and return true.
pub fn focus_running() -> bool {
    unsafe extern "system" fn each(h: HWND, found: LPARAM) -> i32 {
        let mut buf = [0u16; 64];
        let n = unsafe { GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32) };
        if String::from_utf16_lossy(&buf[..n.max(0) as usize]) != TITLE {
            return 1;
        }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(h, &mut pid) };
        if pid == std::process::id() || !is_this_app(pid) {
            return 1;
        }
        unsafe { *(found as *mut HWND) = h };
        0
    }
    let mut found: HWND = std::ptr::null_mut();
    unsafe { EnumWindows(Some(each), &mut found as *mut HWND as LPARAM) };
    if found.is_null() {
        return false;
    }
    show(found);
    true
}

/// Whether process `pid` runs an executable with this app's file name (a folder window named
/// AgentUsage in Explorer must not count).
fn is_this_app(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    let Some(mine) = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_owned()))
    else {
        return false;
    };
    unsafe {
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if p.is_null() {
            return false;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(p);
        ok != 0
            && std::path::Path::new(&String::from_utf16_lossy(&buf[..len as usize]))
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case(&mine))
    }
}
