//! exec.rs — one hidden, bounded, cancellable child process.
//!
//! Every external engine goes through this module. The rules are uniform:
//! no shell, an argv array (never a concatenated command line), stdin closed,
//! stdout/stderr drained with a hard size cap, a private working directory, a
//! below-normal priority class, a per-job timeout and a Job Object with
//! KILL_ON_JOB_CLOSE so cancellation takes the whole child tree down.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::Win32::System::Threading::{
    SetPriorityClass, WaitForSingleObject, BELOW_NORMAL_PRIORITY_CLASS,
};

use super::{CancelToken, EngineError};

/// A size cap for the text an engine can push back at us.
pub const OUTPUT_CAP: usize = 256 * 1024;
const POLL: u32 = 50;

pub struct Spec<'a> {
    pub exe: &'a Path,
    pub args: &'a [OsString],
    pub dir: &'a Path,
    pub timeout: Duration,
}

pub struct Captured {
    pub stdout: String,
    pub stderr: String,
}

/// Runs `spec.exe` to completion. `on_stdout` sees stdout lines as they are
/// produced (ffmpeg's `-progress pipe:1`); pass `None` for tools whose stdout
/// carries no progress.
pub fn run(
    spec: &Spec<'_>,
    cancel: &CancelToken,
    mut on_stdout: impl FnMut(&str),
) -> Result<Captured, EngineError> {
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }

    let job = JobHandle::new()?;
    let mut command = Command::new(spec.exe);
    command
        .args(spec.args)
        .current_dir(spec.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW.0);
    let mut child = command
        .spawn()
        .map_err(|error| EngineError::Spawn(format!("{}: {error}", spec.exe.display())))?;

    let process = HANDLE(child.as_raw_handle());
    // Assigning immediately keeps every descendant the engine creates inside
    // the job. None of the three bundled engines re-execs another program.
    // SAFETY: handles are valid for the child's lifetime.
    let _ = unsafe { AssignProcessToJobObject(job.0, process) };
    // SAFETY: the process handle belongs to `child`.
    let _ = unsafe { SetPriorityClass(process, BELOW_NORMAL_PRIORITY_CLASS) };

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| EngineError::Spawn("missing stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| EngineError::Spawn("missing stderr".into()))?;

    let (line_tx, line_rx) = mpsc::sync_channel::<String>(64);
    let stdout_reader = std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if kept.len() < OUTPUT_CAP {
                        let room = OUTPUT_CAP - kept.len();
                        kept.extend_from_slice(&line.as_bytes()[..read.min(room)]);
                    }
                    let trimmed = line.trim_end_matches(['\r', '\n']).to_owned();
                    if line_tx.send(trimmed).is_err() {
                        // The runner stopped listening; keep draining so the
                        // engine can never block on a full pipe.
                        continue;
                    }
                }
            }
        }
        kept
    });
    let stderr_reader = read_capped(stderr);

    let start = Instant::now();
    let deadline = start + spec.timeout;
    let mut cancelled = false;
    let mut timed_out = false;
    let mut stdout_seen = Vec::new();
    loop {
        while let Ok(line) = line_rx.try_recv() {
            if stdout_seen.len() < OUTPUT_CAP {
                stdout_seen.extend_from_slice(line.as_bytes());
                stdout_seen.push(b'\n');
            }
            on_stdout(&line);
        }
        // SAFETY: the handle is the live child process.
        let waited = unsafe { WaitForSingleObject(process, POLL) };
        if waited == WAIT_OBJECT_0 {
            break;
        }
        if cancel.is_cancelled() {
            cancelled = true;
            let _ = unsafe { TerminateJobObject(job.0, 1) };
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = unsafe { TerminateJobObject(job.0, 2) };
            break;
        }
    }
    let status = child.wait().ok();
    // Drain whatever the reader buffered before the process ended.
    while let Ok(line) = line_rx.try_recv() {
        if stdout_seen.len() < OUTPUT_CAP {
            stdout_seen.extend_from_slice(line.as_bytes());
            stdout_seen.push(b'\n');
        }
        on_stdout(&line);
    }
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();

    if cancelled {
        return Err(EngineError::Cancelled);
    }
    if timed_out {
        return Err(EngineError::Timeout);
    }
    if status.is_some_and(|status| !status.success()) {
        let message = String::from_utf8_lossy(&stderr).trim().to_owned();
        let message = if message.is_empty() {
            format!("{} exited with {}", spec.exe.display(), status.unwrap())
        } else {
            message
        };
        return Err(EngineError::Failed(tail(&message)));
    }

    Ok(Captured {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// Engines can print a lot on failure; the card only has room for the last
/// useful line.
fn tail(message: &str) -> String {
    let line = message
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .next_back()
        .unwrap_or(message);
    line.chars().take(280).collect()
}

struct JobHandle(HANDLE);

impl JobHandle {
    fn new() -> Result<Self, EngineError> {
        // SAFETY: no attributes and no name; the handle is owned here.
        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|error| EngineError::Spawn(format!("job object: {error}")))?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the structure is initialized and the size matches.
        let result = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if result.is_err() {
            let _ = unsafe { CloseHandle(handle) };
            return Err(EngineError::Spawn("job object limits failed".into()));
        }
        Ok(Self(handle))
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        // Closing a job with KILL_ON_JOB_CLOSE terminates anything still in
        // it, which is exactly the cleanup a cancelled job needs.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn read_capped(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if kept.len() < OUTPUT_CAP {
                        let take = (OUTPUT_CAP - kept.len()).min(read);
                        kept.extend_from_slice(&chunk[..take]);
                    }
                }
            }
        }
        kept
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn cmd() -> PathBuf {
        PathBuf::from(
            std::env::var("COMSPEC").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into()),
        )
    }

    fn argv(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    #[test]
    fn output_is_captured_and_exit_codes_are_errors() {
        let dir = std::env::temp_dir();
        let args = argv(&["/C", "echo hello"]);
        let captured = run(
            &Spec {
                exe: &cmd(),
                args: &args,
                dir: &dir,
                timeout: Duration::from_secs(10),
            },
            &CancelToken::new(),
            |_| {},
        )
        .unwrap();
        assert!(captured.stdout.contains("hello"));

        let args = argv(&["/C", "exit 3"]);
        let result = run(
            &Spec {
                exe: &cmd(),
                args: &args,
                dir: &dir,
                timeout: Duration::from_secs(10),
            },
            &CancelToken::new(),
            |_| {},
        );
        assert!(matches!(result, Err(EngineError::Failed(_))));
    }

    #[test]
    fn cancel_terminates_the_child_and_returns_quickly() {
        let dir = std::env::temp_dir();
        // `ping -n 30` on a loopback address keeps the process alive without
        // any filesystem or network traffic leaving the machine.
        let args = argv(&["/C", "ping -n 30 127.0.0.1 > nul"]);
        let cancel = CancelToken::new();
        let flag = cancel.clone();
        let started = Instant::now();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            flag.cancel();
        });
        let result = run(
            &Spec {
                exe: &cmd(),
                args: &args,
                dir: &dir,
                timeout: Duration::from_secs(60),
            },
            &cancel,
            |_| {},
        );
        assert!(matches!(result, Err(EngineError::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn timeout_kills_a_stuck_process() {
        let dir = std::env::temp_dir();
        let args = argv(&["/C", "ping -n 30 127.0.0.1 > nul"]);
        let started = Instant::now();
        let result = run(
            &Spec {
                exe: &cmd(),
                args: &args,
                dir: &dir,
                timeout: Duration::from_millis(200),
            },
            &CancelToken::new(),
            |_| {},
        );
        assert!(matches!(result, Err(EngineError::Timeout)));
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
