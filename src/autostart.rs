//! "Start with Windows": a value under the user's Run key that opens the app in the tray at sign-in.
//! Off unless the user turns it on in the menu. The Run key is the single source of truth, so a
//! value removed in Task Manager's Startup apps reads as off here too.

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};

/// Starts the app hidden in the tray instead of opening its window.
pub const TRAY_ARG: &str = "--tray";

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Its own value name, apart from the "AgentUsage" value some setups use for `--background`.
const VALUE: &str = "AgentUsage (tray)";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Whether Windows starts the app at sign-in.
pub fn enabled() -> bool {
    enabled_as(VALUE)
}

fn enabled_as(name: &str) -> bool {
    let (key, value) = (wide(RUN_KEY), wide(name));
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    status == ERROR_SUCCESS
}

/// Turn starting at sign-in on (pointing at this exe) or off. Returns whether it worked.
pub fn set(on: bool) -> bool {
    set_as(VALUE, on)
}

fn set_as(name: &str, on: bool) -> bool {
    let (key, value) = (wide(RUN_KEY), wide(name));
    if !on {
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
        return status == ERROR_SUCCESS || !enabled_as(name);
    }
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let data = wide(&format!("\"{}\" {TRAY_ARG}", exe.display()));
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
    status == ERROR_SUCCESS
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
}
