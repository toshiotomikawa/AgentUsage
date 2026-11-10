//! Run a console program and capture its output without any window ever appearing.
//!
//! On Windows, `CREATE_NO_WINDOW` hides only the program itself, and a console that merely starts
//! hidden can still flash on screen for an instant. Instead the program runs inside a
//! pseudoconsole, a console with no window at all, which every helper inherits. A job object ties
//! the helpers' lifetime to the call, so none is left behind and the output pipe always closes.

use std::path::Path;
use std::time::Duration;

/// Output beyond this is not a usage report; the run fails instead of growing without bound.
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;

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
        let _ = (&mut out).take(MAX_OUTPUT + 1).read_to_string(&mut s);
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
    use std::ffi::c_void;
    use std::fs::File;
    use std::io::Read;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::Console::{
        COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CREATE_SUSPENDED, CreateProcessW, DeleteProcThreadAttributeList,
        EXTENDED_STARTUPINFO_PRESENT, InitializeProcThreadAttributeList,
        PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, ResumeThread,
        STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute,
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
    /// Headless console: conhost runs without a window, so nothing can flash.
    struct Pty(HPCON);
    impl Drop for Pty {
        fn drop(&mut self) {
            unsafe { ClosePseudoConsole(self.0) };
        }
    }
    struct Attrs(*mut c_void);
    impl Drop for Attrs {
        fn drop(&mut self) {
            unsafe { DeleteProcThreadAttributeList(self.0) };
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
        let reader_file = File::from_raw_handle(read as _);
        if SetHandleInformation(read, HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err("pipe".into());
        }

        // Pseudoconsole: the program and every helper share one console that has no window.
        // What it draws is drained and dropped; stdout still goes to the pipe above.
        let (mut in_r, mut in_w): (HANDLE, HANDLE) = (null_mut(), null_mut());
        if CreatePipe(&mut in_r, &mut in_w, null(), 0) == 0 {
            return Err("pipe".into());
        }
        let (in_r, _in_w) = (Owned(in_r), Owned(in_w));
        let (mut scr_r, mut scr_w): (HANDLE, HANDLE) = (null_mut(), null_mut());
        if CreatePipe(&mut scr_r, &mut scr_w, null(), 0) == 0 {
            return Err("pipe".into());
        }
        let scr_w = Owned(scr_w);
        let mut screen = File::from_raw_handle(scr_r as _);
        let mut hpc: HPCON = 0;
        if CreatePseudoConsole(COORD { X: 200, Y: 50 }, in_r.0, scr_w.0, 0, &mut hpc) < 0 {
            return Err("console".into());
        }
        let pty = Pty(hpc);
        // The pseudoconsole keeps its own copies of these ends.
        drop(in_r);
        drop(scr_w);
        let drain = std::thread::spawn(move || {
            let _ = std::io::copy(&mut screen, &mut std::io::sink());
        });

        let mut size = 0usize;
        InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
        let mut buf = vec![0u8; size.max(1)];
        let list = buf.as_mut_ptr() as *mut c_void;
        if InitializeProcThreadAttributeList(list, 1, 0, &mut size) == 0 {
            return Err("console".into());
        }
        let attrs = Attrs(list);
        if UpdateProcThreadAttribute(
            attrs.0,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            pty.0 as *const c_void,
            std::mem::size_of::<HPCON>(),
            null_mut(),
            null(),
        ) == 0
        {
            return Err("console".into());
        }

        // Job: closing it ends every process the program started.
        // Without it a helper could outlive the call and hold the pipe open, so no job, no run.
        let job = Owned(CreateJobObjectW(null(), null()));
        if job.0.is_null() {
            return Err("job".into());
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            return Err("job".into());
        }

        let mut si: STARTUPINFOEXW = std::mem::zeroed();
        si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.StartupInfo.hStdOutput = write.0;
        si.lpAttributeList = attrs.0;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            null(),
            line.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED,
            null(),
            cwd_w.as_ptr(),
            &si.StartupInfo,
            &mut pi,
        );
        if ok == 0 {
            return Err("not found".into());
        }
        let process = Owned(pi.hProcess);
        let thread = Owned(pi.hThread);
        // Still suspended: nothing has run yet if either step fails.
        if AssignProcessToJobObject(job.0, process.0) == 0 || ResumeThread(thread.0) == u32::MAX {
            TerminateProcess(process.0, 1);
            return Err("job".into());
        }
        // Only the child (and its helpers) hold the write end now.
        drop(write);

        let reader = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = reader_file.take(MAX_OUTPUT + 1).read_to_string(&mut s);
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
        // Closing the console ends what it draws, so the drain finishes too.
        drop(attrs);
        drop(pty);
        let _ = drain.join();
        if out.len() as u64 > MAX_OUTPUT {
            return Err("failed".into());
        }
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
