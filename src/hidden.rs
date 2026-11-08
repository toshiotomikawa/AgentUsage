//! Run a console program and capture its output without any window ever appearing.
//!
//! On Windows, `CREATE_NO_WINDOW` hides only the program itself: helper processes it starts get a
//! fresh, visible console each (terminals flashing on screen). Instead the program gets a console
//! of its own that starts hidden, which every helper inherits. A job object ties the helpers'
//! lifetime to the call, so none is left behind and the output pipe always closes.

use std::path::Path;
use std::time::Duration;

#[cfg(not(windows))]
pub fn run(
    exe: &std::ffi::OsStr,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
) -> Result<String, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new(exe)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "not found".to_string())?;
    let mut out = child.stdout.take().ok_or("no output")?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("timeout".into());
            }
        }
    }
    reader.join().map_err(|_| "failed".to_string())
}

#[cfg(windows)]
pub fn run(
    exe: &std::ffi::OsStr,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
) -> Result<String, String> {
    use std::fs::File;
    use std::io::Read;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CREATE_NEW_CONSOLE, CREATE_SUSPENDED, CreateProcessW, PROCESS_INFORMATION, ResumeThread,
        STARTF_USESHOWWINDOW, STARTF_USESTDHANDLES, STARTUPINFOW, TerminateProcess,
        WaitForSingleObject,
    };

    struct Owned(HANDLE);
    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain([0]).collect() };
    let mut line: Vec<u16> = {
        let mut s = std::ffi::OsString::from("\"");
        s.push(exe);
        s.push("\"");
        for a in args {
            s.push(" ");
            if a.contains(' ') {
                s.push(format!("\"{a}\""));
            } else {
                s.push(a);
            }
        }
        wide(&s)
    };
    let cwd_w = wide(cwd.as_os_str());

    unsafe {
        // Output pipe: the child writes to an inheritable end, we read the other.
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let (mut read, mut write): (HANDLE, HANDLE) = (null_mut(), null_mut());
        if CreatePipe(&mut read, &mut write, &sa, 0) == 0 {
            return Err("pipe".into());
        }
        let write = Owned(write);
        SetHandleInformation(read, HANDLE_FLAG_INHERIT, 0);
        let reader_file = File::from_raw_handle(read as _);

        // Job: closing it ends every process the program started.
        let job = Owned(CreateJobObjectW(null(), null()));
        if !job.0.is_null() {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
        }

        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESHOWWINDOW | STARTF_USESTDHANDLES;
        si.wShowWindow = 0; // SW_HIDE
        si.hStdOutput = write.0;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            null(),
            line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_NEW_CONSOLE | CREATE_SUSPENDED,
            null(),
            cwd_w.as_ptr(),
            &si,
            &mut pi,
        );
        if ok == 0 {
            return Err("not found".into());
        }
        let process = Owned(pi.hProcess);
        let thread = Owned(pi.hThread);
        if !job.0.is_null() {
            AssignProcessToJobObject(job.0, process.0);
        }
        ResumeThread(thread.0);
        // Only the child (and its helpers) hold the write end now.
        drop(write);

        let reader = std::thread::spawn(move || {
            let mut s = String::new();
            let mut f = reader_file;
            let _ = f.read_to_string(&mut s);
            s
        });
        let ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        let finished = WaitForSingleObject(process.0, ms) == WAIT_OBJECT_0;
        if !finished {
            TerminateProcess(process.0, 1);
        }
        // Ends any helper still running, which also closes the pipe for the reader.
        drop(job);
        let out = reader.join().map_err(|_| "failed".to_string())?;
        if finished {
            Ok(out)
        } else {
            Err("timeout".into())
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn captures_output_of_nested_helpers() {
        let cmd = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
        let out = super::run(
            &cmd,
            &["/c", "cmd", "/c", "echo", "hidden-ok"],
            &std::env::temp_dir(),
            std::time::Duration::from_secs(10),
        )
        .unwrap();
        assert!(out.contains("hidden-ok"));
    }
}
