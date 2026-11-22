//! "Start with Windows": a value under the user's Run key that opens the app in the tray at sign-in.
//! Off unless the user turns it on in the menu. It reads as on only when Windows will really start
//! this exe: the value points at this exe (not a copy moved away) and Task Manager's Startup apps
//! has not disabled it.

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
    RegSetKeyValueW,
};

/// Starts the app hidden in the tray instead of opening its window.
pub const TRAY_ARG: &str = "--tray";

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Where Task Manager's Startup apps marks a Run value disabled (first byte odd) or enabled.
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// Its own value name, apart from the "AgentUsage" value some setups use for `--background`.
const VALUE: &str = "AgentUsage (tray)";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The command line the Run value holds for this exe.
fn command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" {TRAY_ARG}", exe.display()))
}

/// Whether Windows starts the app at sign-in.
pub fn enabled() -> bool {
    enabled_as(VALUE)
}

fn enabled_as(name: &str) -> bool {
    let (Some(want), Some(have)) = (command(), read_text(name)) else {
        return false;
    };
    have.eq_ignore_ascii_case(&want) && !disabled_in_task_manager(name)
}

fn read_text(name: &str) -> Option<String> {
    let (key, value) = (wide(RUN_KEY), wide(name));
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    // The length counts bytes, the closing NUL included.
    let chars = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars]))
}

fn disabled_in_task_manager(name: &str) -> bool {
    let (key, value) = (wide(APPROVED_KEY), wide(name));
    let mut buf = [0u8; 16];
    let mut len = buf.len() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_BINARY,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut len,
        )
    };
    status == ERROR_SUCCESS && len > 0 && buf[0] & 1 == 1
}

/// Turn starting at sign-in on (pointing at this exe) or off. Returns whether it worked.
pub fn set(on: bool) -> bool {
    set_as(VALUE, on)
}

fn set_as(name: &str, on: bool) -> bool {
    let (key, value) = (wide(RUN_KEY), wide(name));
    if !on {
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
        return status == ERROR_SUCCESS || read_text(name).is_none();
    }
    let Some(cmd) = command() else {
        return false;
    };
    let data = wide(&cmd);
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
    // Turning it on here also lifts a "Disabled" set earlier in Task Manager.
    let approved = wide(APPROVED_KEY);
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, approved.as_ptr(), value.as_ptr()) };
    status == ERROR_SUCCESS && enabled_as(name)
}

#[cfg(test)]
mod tests {
    // Uses a value name of its own, so the app's real entries are never touched.
    #[test]
    fn turns_on_and_off() {
        let name = "AgentUsage (test)";
        assert!(super::set_as(name, true));
        assert!(super::enabled_as(name));
        assert!(super::set_as(name, false));
        assert!(!super::enabled_as(name));
        assert!(
            super::set_as(name, false),
            "turning off twice still succeeds"
        );
    }

    #[test]
    fn reads_off_when_moved_or_disabled() {
        use super::*;
        let name = "AgentUsage (test moved)";
        let (key, value, approved) = (wide(RUN_KEY), wide(name), wide(APPROVED_KEY));
        let elsewhere = wide(&format!(r#""C:\elsewhere\agent-usage.exe" {TRAY_ARG}"#));
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                REG_SZ,
                elsewhere.as_ptr().cast(),
                (elsewhere.len() * 2) as u32,
            )
        };
        assert!(!enabled_as(name), "a value for a moved exe reads as off");
        assert!(set_as(name, true), "turning on points it at this exe");
        let off = [3u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                approved.as_ptr(),
                value.as_ptr(),
                windows_sys::Win32::System::Registry::REG_BINARY,
                off.as_ptr().cast(),
                off.len() as u32,
            )
        };
        assert!(!enabled_as(name), "disabled in Task Manager reads as off");
        assert!(set_as(name, true), "turning on lifts the Task Manager flag");
        assert!(set_as(name, false));
    }
}
