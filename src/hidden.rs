//! Run a console program and capture its output without any window ever appearing.
//!
//! On Windows, `CREATE_NO_WINDOW` hides only the program itself, and a console that merely starts
//! hidden can still flash on screen for an instant. Instead the program runs inside a
//! pseudoconsole, a console with no window at all, which every helper inherits. A job object ties
//! the helpers' lifetime to the call, so none is left behind and the output pipe always closes.

use std::ffi::OsStr;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Output beyond this is not a usage report; the run fails instead of growing without bound.
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;

/// How long the output may take to finish arriving once the program has ended.
#[cfg(windows)]
const OUTPUT_GRACE: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RunError {
    NotFound,
    Timeout,
    Aborted,
    Failed,
}

/// Read a pipe up to `MAX_OUTPUT + 1` bytes, setting `aborted` as soon as `needle` appears.
fn read_stream(mut reader: impl Read, needle: Option<String>, aborted: Arc<AtomicBool>) -> String {
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let limit = (MAX_OUTPUT + 1) as usize;
    let needle_bytes = needle.as_deref().map(str::as_bytes);
    while raw.len() < limit {
        let want = buf.len().min(limit - raw.len());
        match reader.read(&mut buf[..want]) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                // Search only the new bytes plus enough old ones to catch a match split across reads.
                let from = needle_bytes.map_or(0, |pat| raw.len().saturating_sub(pat.len() - 1));
                raw.extend_from_slice(&buf[..n]);
                if let Some(pat) = needle_bytes
                    && !aborted.load(Ordering::Relaxed)
                    && raw[from..].windows(pat.len()).any(|w| w == pat)
                {
                    aborted.store(true, Ordering::Release);
                }
            }
        }
    }
    String::from_utf8_lossy(&raw).into_owned()
}

#[cfg(not(windows))]
pub fn run(
    exe: &OsStr,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
    env: &[(&str, &OsStr)],
    abort_on_stderr: Option<&str>,
) -> Result<Output, RunError> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|_| RunError::NotFound)?;
    let out = child.stdout.take().ok_or(RunError::Failed)?;
    let err = child.stderr.take().ok_or(RunError::Failed)?;
    let aborted = Arc::new(AtomicBool::new(false));
    let out_reader =
        std::thread::spawn(move || read_stream(out, None, Arc::new(AtomicBool::new(false))));
    let err_aborted = aborted.clone();
    let needle = abort_on_stderr.map(str::to_string);
    let err_reader = std::thread::spawn(move || read_stream(err, needle, err_aborted));

    let deadline = std::time::Instant::now() + timeout;
    let mut did_abort = false;
    let mut finished = false;
    loop {
        if aborted.load(Ordering::Acquire) {
            did_abort = true;
            break;
        }
        match child.try_wait() {
            Ok(Some(_)) => {
                finished = true;
                break;
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100))
            }
            _ => break,
        }
    }
    if !finished {
        let _ = child.kill();
        let _ = child.wait();
    }
    let stdout = out_reader.join().map_err(|_| RunError::Failed)?;
    let stderr = err_reader.join().map_err(|_| RunError::Failed)?;
    if did_abort || aborted.load(Ordering::Acquire) {
        return Err(RunError::Aborted);
    }
    if !finished {
        return Err(RunError::Timeout);
    }
    if stdout.len() as u64 > MAX_OUTPUT || stderr.len() as u64 > MAX_OUTPUT {
        return Err(RunError::Failed);
    }
    Ok(Output { stdout, stderr })
}

#[cfg(windows)]
fn build_env_block(overrides: &[(&str, &OsStr)]) -> Vec<u16> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt;
    let mut vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    for (k, v) in overrides {
        if let Some((_, existing)) = vars
            .iter_mut()
            .find(|(ek, _)| ek.to_string_lossy().eq_ignore_ascii_case(k))
        {
            *existing = v.to_os_string();
        } else {
            vars.push((OsString::from(k), v.to_os_string()));
        }
    }
    // Windows requires Unicode environment blocks to be sorted case-insensitively by name.
    vars.sort_by(|(a, _), (b, _)| {
        a.to_string_lossy()
            .to_ascii_uppercase()
            .cmp(&b.to_string_lossy().to_ascii_uppercase())
    });
    let mut block = Vec::new();
    for (k, v) in vars {
        block.extend(k.encode_wide());
        block.push(b'=' as u16);
        block.extend(v.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

#[cfg(windows)]
pub fn run(
    exe: &OsStr,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
    env: &[(&str, &OsStr)],
    abort_on_stderr: Option<&str>,
) -> Result<Output, RunError> {
    use std::ffi::c_void;
    use std::fs::File;
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
        CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
        DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
        InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
        PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
        UpdateProcThreadAttribute, WaitForSingleObject,
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

    let wide = |s: &OsStr| -> Vec<u16> { s.encode_wide().chain([0]).collect() };
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
    let env_block = (!env.is_empty()).then(|| build_env_block(env));

    unsafe {
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        // Output and error pipes: the child writes to inheritable ends, we read the others.
        let (mut out_r, mut out_w): (HANDLE, HANDLE) = (null_mut(), null_mut());
        if CreatePipe(&mut out_r, &mut out_w, &sa, 0) == 0 {
            return Err(RunError::Failed);
        }
        let out_w = Owned(out_w);
        let out_file = File::from_raw_handle(out_r as _);
        if SetHandleInformation(out_r, HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(RunError::Failed);
        }

        let (mut err_r, mut err_w): (HANDLE, HANDLE) = (null_mut(), null_mut());
        if CreatePipe(&mut err_r, &mut err_w, &sa, 0) == 0 {
            return Err(RunError::Failed);
        }
        let err_w = Owned(err_w);
        let err_file = File::from_raw_handle(err_r as _);
        if SetHandleInformation(err_r, HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(RunError::Failed);
        }

        // Pseudoconsole: the program and every helper share one console that has no window.
        // What it draws is drained and dropped; stdout/stderr still go to the pipes above.
        let (mut in_r, mut in_w): (HANDLE, HANDLE) = (null_mut(), null_mut());
        if CreatePipe(&mut in_r, &mut in_w, null(), 0) == 0 {
            return Err(RunError::Failed);
        }
        let (in_r, _in_w) = (Owned(in_r), Owned(in_w));
        let (mut scr_r, mut scr_w): (HANDLE, HANDLE) = (null_mut(), null_mut());
        if CreatePipe(&mut scr_r, &mut scr_w, null(), 0) == 0 {
            return Err(RunError::Failed);
        }
        let scr_w = Owned(scr_w);
        let mut screen = File::from_raw_handle(scr_r as _);
        let mut hpc: HPCON = 0;
        if CreatePseudoConsole(COORD { X: 200, Y: 50 }, in_r.0, scr_w.0, 0, &mut hpc) < 0 {
            return Err(RunError::Failed);
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
            return Err(RunError::Failed);
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
            return Err(RunError::Failed);
        }

        // Job: closing it ends every process the program started.
        // Without it a helper could outlive the call and hold the pipe open, so no job, no run.
        let job = Owned(CreateJobObjectW(null(), null()));
        if job.0.is_null() {
            return Err(RunError::Failed);
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
            return Err(RunError::Failed);
        }

        let mut si: STARTUPINFOEXW = std::mem::zeroed();
        si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.StartupInfo.hStdOutput = out_w.0;
        si.StartupInfo.hStdError = err_w.0;
        si.lpAttributeList = attrs.0;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let mut flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED;
        let env_ptr = match &env_block {
            Some(b) => {
                flags |= CREATE_UNICODE_ENVIRONMENT;
                b.as_ptr() as *const c_void
            }
            None => null(),
        };
        let ok = CreateProcessW(
            null(),
            line.as_mut_ptr(),
            null(),
            null(),
            1,
            flags,
            env_ptr,
            cwd_w.as_ptr(),
            &si.StartupInfo,
            &mut pi,
        );
        if ok == 0 {
            return Err(RunError::NotFound);
        }
        let process = Owned(pi.hProcess);
        let thread = Owned(pi.hThread);
        // Still suspended: nothing has run yet if either step fails.
        if AssignProcessToJobObject(job.0, process.0) == 0 || ResumeThread(thread.0) == u32::MAX {
            TerminateProcess(process.0, 1);
            return Err(RunError::Failed);
        }
        // Only the child (and its helpers) hold the write ends now.
        drop(out_w);
        drop(err_w);

        let aborted = Arc::new(AtomicBool::new(false));
        let (out_tx, out_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let s = read_stream(out_file, None, Arc::new(AtomicBool::new(false)));
            let _ = out_tx.send(s);
        });
        let (err_tx, err_rx) = std::sync::mpsc::channel();
        let err_aborted = aborted.clone();
        let needle = abort_on_stderr.map(str::to_string);
        std::thread::spawn(move || {
            let s = read_stream(err_file, needle, err_aborted);
            let _ = err_tx.send(s);
        });

        let deadline = std::time::Instant::now() + timeout;
        let mut finished = false;
        let mut did_abort = false;
        loop {
            if aborted.load(Ordering::Acquire) {
                did_abort = true;
                break;
            }
            if WaitForSingleObject(process.0, 100) == WAIT_OBJECT_0 {
                finished = true;
                break;
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
        }
        if !finished {
            TerminateProcess(process.0, 1);
        }
        // Ends any helper still running, which also closes the pipes for the readers. A process
        // that left the job could still hold them open, so the wait for the output is bounded:
        // waiting on it forever froze a source for good.
        drop(job);
        let out = out_rx.recv_timeout(OUTPUT_GRACE);
        let err = err_rx.recv_timeout(OUTPUT_GRACE);
        // Closing the console ends what it draws, so the drain finishes on its own; it is not
        // waited for, for the same reason.
        drop(attrs);
        drop(pty);
        drop(drain);
        if did_abort || aborted.load(Ordering::Acquire) {
            return Err(RunError::Aborted);
        }
        let (Ok(stdout), Ok(stderr)) = (out, err) else {
            return Err(RunError::Timeout);
        };
        if stdout.len() as u64 > MAX_OUTPUT || stderr.len() as u64 > MAX_OUTPUT {
            return Err(RunError::Failed);
        }
        if finished {
            Ok(Output { stdout, stderr })
        } else {
            Err(RunError::Timeout)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn captures_output_of_nested_helpers() {
        let cmd = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
        let out = run(
            &cmd,
            &["/c", "cmd", "/c", "echo", "hidden-ok"],
            &std::env::temp_dir(),
            Duration::from_secs(10),
            &[],
            None,
        )
        .unwrap();
        assert!(out.stdout.contains("hidden-ok"));
    }

    #[cfg(windows)]
    #[test]
    fn env_override_and_stderr_abort() {
        let cmd = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
        let val = OsStr::new("custom-child-value");
        let out = run(
            &cmd,
            &["/c", "echo", "%AGENT_USAGE_TEST_ENV%"],
            &std::env::temp_dir(),
            Duration::from_secs(10),
            &[("AGENT_USAGE_TEST_ENV", val)],
            None,
        )
        .unwrap();
        assert!(out.stdout.contains("custom-child-value"));
        assert!(std::env::var_os("AGENT_USAGE_TEST_ENV").is_none());

        let started = std::time::Instant::now();
        let err = run(
            &cmd,
            &[
                "/c",
                "echo Authentication required. Please visit the URL 1>&2 & ping -n 20 127.0.0.1 >nul",
            ],
            &std::env::temp_dir(),
            Duration::from_secs(15),
            &[],
            Some("Authentication required"),
        )
        .unwrap_err();
        assert_eq!(err, RunError::Aborted);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
