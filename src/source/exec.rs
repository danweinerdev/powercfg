//! Bounded subprocess execution with a hard timeout.
//!
//! Source modules that shell out (pactl, journalctl, dmesg) wrap their
//! `Command` invocation here so a hung subprocess can't block the CLI
//! indefinitely. The wait-timeout crate handles the kernel-level wait;
//! callers handle the parsing.

use std::process::{Command, Output};
use std::time::Duration;

use thiserror::Error;

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
/// The caller is responsible for setting up the `Command` (program,
/// args, env). This helper sets `stdin(Stdio::null())`,
/// `stdout(Stdio::piped())`, and `stderr(Stdio::piped())` itself.
// TODO(phase-3.3): first production caller is source::userspace::list_audio_streams.
#[allow(dead_code)]
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Result<Output, ExecError> {
    use std::io::Read;
    use std::process::Stdio;
    use wait_timeout::ChildExt;

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

    let status = match child.wait_timeout(timeout)? {
        Some(s) => s,
        None => {
            // Timed out — kill and reap so the child doesn't outlive us.
            // wait() always returns the status; we discard it because
            // we already know what we want to report.
            let _ = child.kill();
            let _ = child.wait();
            return Err(ExecError::Timeout { timeout });
        }
    };

    // Drain stdout/stderr that the child wrote before exiting.
    let mut stdout_buf = Vec::new();
    if let Some(mut s) = child.stdout.take() {
        s.read_to_end(&mut stdout_buf)?;
    }
    let mut stderr_buf = Vec::new();
    if let Some(mut s) = child.stderr.take() {
        s.read_to_end(&mut stderr_buf)?;
    }

    Ok(Output {
        status,
        stdout: stdout_buf,
        stderr: stderr_buf,
    })
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
}
