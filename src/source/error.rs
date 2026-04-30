//! Typed error enum returned by every source function.
//!
//! See `Designs/RustRewrite/README.md` Decision 7 for the rationale: source
//! functions return `Result<T, SourceError>` and the command layer applies
//! `.unwrap_or_default()` / `.ok()` per call site, so failures stay typed
//! and observable at `RUST_LOG=debug` while the user-facing fallback
//! ("None.") matches the Python tool's `try/except: pass` behavior.

use thiserror::Error;

/// Failure modes for source-layer functions.
///
/// `Dbus`, `Subprocess`, and `Timeout` carry placeholder `String` payloads
/// during Phase 1; Phase 3 swaps them for richer typed payloads
/// (`zbus::Error` arrives in 3.2 and the in-tree `ExecError` in 3.1)
/// without requiring callers to change shape.
#[derive(Debug, Error)]
pub enum SourceError {
    /// Filesystem I/O error (sysfs/procfs read).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Parsed bytes did not match the expected shape.
    #[error("parse: {0}")]
    Parse(String),

    /// Expected file/path/device was absent on this system.
    #[error("not found: {0}")]
    NotFound(String),

    /// D-Bus call failed.
    ///
    /// Phase 3.2 changes the payload to `zbus::Error`; the variant is
    /// declared now so Phase 2 can return `Result<T, SourceError>` from
    /// sysfs/procfs sources without churning the enum later.
    #[error("dbus: {0}")]
    Dbus(String),

    /// Subprocess invocation failed (spawn, non-zero exit, decode).
    ///
    /// Phase 3.1 changes the payload to the in-tree `ExecError`; declared
    /// now for the same reason as `Dbus`.
    #[error("subprocess: {0}")]
    Subprocess(String),

    /// Subprocess exceeded its bounded timeout.
    ///
    /// Filled in when Phase 3.1 lands `source::exec::run_with_timeout`.
    #[error("timeout: {0}")]
    Timeout(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_variant_displays_with_io_prefix() {
        let raw = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = SourceError::Io(raw);
        assert_eq!(err.to_string(), "io: denied");
    }

    #[test]
    fn parse_variant_displays_with_parse_prefix() {
        let err = SourceError::Parse("expected 2 fields, got 1".into());
        assert_eq!(err.to_string(), "parse: expected 2 fields, got 1");
    }

    #[test]
    fn not_found_variant_displays_with_not_found_prefix() {
        let err = SourceError::NotFound("/sys/power/disk".into());
        assert_eq!(err.to_string(), "not found: /sys/power/disk");
    }

    #[test]
    fn dbus_variant_displays_with_dbus_prefix() {
        let err = SourceError::Dbus("connection refused".into());
        assert_eq!(err.to_string(), "dbus: connection refused");
    }

    #[test]
    fn subprocess_variant_displays_with_subprocess_prefix() {
        let err = SourceError::Subprocess("journalctl exited 1".into());
        assert_eq!(err.to_string(), "subprocess: journalctl exited 1");
    }

    #[test]
    fn timeout_variant_displays_with_timeout_prefix() {
        let err = SourceError::Timeout("dmesg timed out after 5s".into());
        assert_eq!(err.to_string(), "timeout: dmesg timed out after 5s");
    }

    #[test]
    fn from_io_error_via_question_mark() {
        fn read() -> Result<(), SourceError> {
            // ? should convert std::io::Error → SourceError::Io via #[from].
            let _ = std::fs::read_to_string("/definitely/does/not/exist/powercfg-test")?;
            Ok(())
        }
        let err = read().expect_err("file should not exist");
        assert!(
            matches!(err, SourceError::Io(_)),
            "expected SourceError::Io, got {err:?}",
        );
        assert!(err.to_string().starts_with("io: "));
    }
}
