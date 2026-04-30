//! Bounded subprocess execution with a hard timeout.
//!
//! Source modules that shell out (pactl, journalctl, dmesg) wrap their
//! `Command` invocation here so a hung subprocess can't block the CLI
//! indefinitely. The wait-timeout crate handles the kernel-level wait;
//! callers handle the parsing.
//!
//! ## Pipe-buffer note
//!
//! Stdin is closed (`Stdio::null()`); stdout and stderr are captured.
//! To avoid the 64 KB pipe-buffer deadlock that `Child::wait_with_output`
//! avoids by spawning concurrent reader threads, this helper does the
//! same: two threads drain the pipes while the foreground waits on
//! `wait_timeout`. Without that, a child producing more than ~64 KB
//! before exiting would block in `write()`, the pipe wouldn't drain,
//! and the wait would hang. Phase 4's `journalctl` caller produces
//! megabytes of output; the threaded drain is what keeps it correct.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use thiserror::Error;
use wait_timeout::ChildExt;

/// Reasons a bounded execution can fail.
///
/// These convert into `SourceError::Subprocess(...)` at the source-module
/// boundary (via `#[from]` on `SourceError::Subprocess`) so callers see a
/// typed reason rather than a stringified io error.
///
/// `NotFound` and `Timeout` are reachable through `run_with_timeout` and
/// covered by this module's tests, but no production caller hits them
/// until tasks 3.3 (`source::userspace`) and 3.4 (`cmd::requests`) wire
/// the helper into command paths — hence the `dead_code` allow.
// TODO(phase-3.3): variants reached via source::userspace::list_audio_streams.
#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum ExecError {
    /// The binary couldn't be found on `$PATH`. Distinct from `Io` so
    /// callers can fall back gracefully (e.g., `pactl` on a system
    /// without PulseAudio installed).
    #[error("binary not found: {0}")]
    NotFound(String),

    /// The child process exceeded `timeout` and was killed.
    #[error("process exceeded {timeout:?} timeout")]
    Timeout { timeout: Duration },

    /// Any other I/O failure during spawn/wait/read.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Spawn `cmd` and wait at most `timeout` for it to finish. Returns
/// the captured `Output` on success, an `ExecError::Timeout` if the
/// child outlasts the timeout (the child is killed and reaped before
/// returning), `ExecError::NotFound` if the binary isn't on PATH, and
/// `ExecError::Io` for other failures.
///
/// The caller supplies the `Command` (program, args, env). This helper
/// **always overrides** stdin/stdout/stderr: stdin is `Stdio::null()`,
/// stdout and stderr are captured via piped readers drained concurrently
/// by background threads. Any stdio configured on `cmd` before calling
/// is replaced.
///
/// On a successful run the captured stdout/stderr are returned in
/// `Output`. On `ExecError::Timeout`, the child is killed before this
/// function returns, but the partial pipe contents are discarded — the
/// caller can't recover stdout written before the kill. On
/// `ExecError::Io` from `wait_timeout`, the pipes are similarly closed
/// without draining.
// TODO(phase-3.3): first production caller is source::userspace::list_audio_streams.
#[allow(dead_code)]
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Result<Output, ExecError> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Try to recover the program name for a friendlier error.
            // std::process::Command exposes get_program() since 1.57.
            let prog = cmd.get_program().to_string_lossy().into_owned();
            return Err(ExecError::NotFound(prog));
        }
        Err(e) => return Err(ExecError::Io(e)),
    };

    // Drain the pipes concurrently so the child can write more than
    // 64 KB without blocking on a full pipe. Mirrors the pattern
    // `Child::wait_with_output` uses internally.
    let stdout_handle = child.stdout.take().map(|mut s| {
        thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut buf = Vec::new();
            s.read_to_end(&mut buf)?;
            Ok(buf)
        })
    });
    let stderr_handle = child.stderr.take().map(|mut s| {
        thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut buf = Vec::new();
            s.read_to_end(&mut buf)?;
            Ok(buf)
        })
    });

    let status = match child.wait_timeout(timeout)? {
        Some(s) => s,
        None => {
            // Timed out — kill and reap so the child doesn't outlive
            // us. Killing is allowed to fail (ESRCH if the child raced
            // to exit before our check) and we drop that error
            // silently; the wait error is logged at debug because a
            // real wait failure would leave a zombie behind.
            let _ = child.kill();
            if let Err(e) = child.wait() {
                tracing::debug!("wait() after kill failed: {e}");
            }
            return Err(ExecError::Timeout { timeout });
        }
    };

    // Join the drain threads. Either pipe absent (rare; the caller
    // can't disable them since we set `Stdio::piped` above) or a
    // joined panic — both folded into "no captured bytes" rather than
    // surfacing as a function-level error.
    let stdout_buf = join_drain(stdout_handle)?;
    let stderr_buf = join_drain(stderr_handle)?;

    Ok(Output {
        status,
        stdout: stdout_buf,
        stderr: stderr_buf,
    })
}

/// Join a pipe-drain thread and unwrap its captured bytes.
///
/// Thread panics surface as `ExecError::Io` with a synthesized
/// `io::ErrorKind::Other`; the actual panic payload isn't recoverable
/// through the `JoinHandle` API.
fn join_drain(
    handle: Option<thread::JoinHandle<std::io::Result<Vec<u8>>>>,
) -> Result<Vec<u8>, ExecError> {
    match handle {
        Some(h) => match h.join() {
            Ok(io_result) => Ok(io_result?),
            Err(_) => Err(ExecError::Io(std::io::Error::other(
                "pipe-drain thread panicked",
            ))),
        },
        None => Ok(Vec::new()),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{Duration, Instant};

    #[test]
    fn fast_success_returns_stdout() {
        let mut cmd = Command::new("echo");
        cmd.arg("hello");
        let out = run_with_timeout(cmd, Duration::from_secs(5)).expect("echo should succeed");
        assert!(
            out.status.success(),
            "echo exited non-zero: {:?}",
            out.status
        );
        assert_eq!(out.stdout, b"hello\n");
    }

    #[test]
    fn timeout_kills_long_running_child() {
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        let timeout = Duration::from_millis(200);
        let started = Instant::now();
        let err = run_with_timeout(cmd, timeout).expect_err("sleep 10 should hit the timeout");
        let elapsed = started.elapsed();

        match err {
            ExecError::Timeout { timeout: t } => {
                assert_eq!(t, timeout);
            }
            other => panic!("expected ExecError::Timeout, got {other:?}"),
        }
        // Generous slack for CI variability — the call must return well
        // before sleep(10) would have completed.
        assert!(
            elapsed < timeout + Duration::from_millis(500),
            "run_with_timeout returned in {elapsed:?}, expected < {:?}",
            timeout + Duration::from_millis(500)
        );
    }

    #[test]
    fn nonexistent_binary_returns_not_found() {
        let cmd = Command::new("nonexistent-binary-xyzzy-please-do-not-exist");
        let err = run_with_timeout(cmd, Duration::from_secs(1))
            .expect_err("missing binary should not succeed");
        match err {
            ExecError::NotFound(name) => {
                assert_eq!(name, "nonexistent-binary-xyzzy-please-do-not-exist");
            }
            other => panic!("expected ExecError::NotFound, got {other:?}"),
        }
    }

    #[test]
    fn stderr_is_captured_separately_from_stdout() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo out; echo err >&2"]);
        let out = run_with_timeout(cmd, Duration::from_secs(5)).expect("sh should succeed");
        assert!(out.status.success());
        assert_eq!(out.stdout, b"out\n");
        assert_eq!(out.stderr, b"err\n");
    }

    #[test]
    fn subsequent_call_after_timeout_is_not_blocked() {
        // Proxy for "the previous child is reaped, not still hanging
        // around as a zombie". If reap is broken, this test will still
        // pass — but combined with the timeout-elapsed assertion above,
        // it ensures the helper returns control to the caller cleanly
        // and doesn't wedge a future call.
        let mut slow = Command::new("sleep");
        slow.arg("10");
        let _ = run_with_timeout(slow, Duration::from_millis(100));

        let mut fast = Command::new("echo");
        fast.arg("after-timeout");
        let out =
            run_with_timeout(fast, Duration::from_secs(5)).expect("subsequent echo should succeed");
        assert_eq!(out.stdout, b"after-timeout\n");
    }

    #[test]
    fn large_stdout_does_not_deadlock() {
        // Exercises the concurrent-drain fix. `yes hello | head -c 200000`
        // produces 200 KB of stdout — well past the 64 KB Linux pipe
        // buffer. Without the threaded drain, the child blocks in
        // write() and we deadlock in wait_timeout. Use a 5s budget and
        // assert we return well within it with the full payload.
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "yes hello | head -c 200000"]);
        let started = Instant::now();
        let out = run_with_timeout(cmd, Duration::from_secs(5))
            .expect("large-output run should not deadlock");
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "should finish quickly, took {elapsed:?}",
        );
        assert_eq!(out.stdout.len(), 200_000);
    }

    #[test]
    fn non_executable_file_returns_io() {
        // Permission-denied on an existing file is the most reliable
        // path to ExecError::Io across platforms. It exercises the
        // generic `Err(e) => Io(e)` arm in `spawn()` (kind != NotFound).
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("notexec");
        std::fs::write(&path, b"#!/bin/sh\necho noop\n").expect("write file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("set perms");
        let cmd = Command::new(&path);
        let err = run_with_timeout(cmd, Duration::from_secs(1))
            .expect_err("non-executable file should fail");
        assert!(
            matches!(err, ExecError::Io(_)),
            "expected ExecError::Io, got {err:?}",
        );
    }
}
