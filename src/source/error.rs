//! Typed error enum returned by every source function.
//!
//! See `Designs/RustRewrite/README.md` Decision 7 for the rationale: source
//! functions return `Result<T, SourceError>` and the command layer applies
//! `.unwrap_or_default()` / `.ok()` per call site, so failures stay typed
//! and observable at `RUST_LOG=debug` while the user-facing fallback
//! ("None.") matches the Python tool's `try/except: pass` behavior.

use thiserror::Error;

use crate::source::exec::ExecError;

/// Failure modes for source-layer functions.
///
/// `Subprocess` wraps the typed `ExecError` from `source::exec`, so a
/// caller that uses `run_with_timeout(...)?` automatically bubbles
/// `NotFound`, `Timeout`, and generic `Io` failures up as
/// `SourceError::Subprocess(_)`. `Dbus` wraps `zbus::Error` so callers
/// that use `?` from any `source::dbus` function bubble cleanly.
#[derive(Debug, Error)]
pub enum SourceError {
    /// Filesystem I/O error (sysfs/procfs read).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Parsed bytes did not match the expected shape.
    #[error("parse: {0}")]
    Parse(String),

    /// Expected file/path/device was absent on this system.
    ///
    /// Source modules use this when "file absent" carries different
    /// meaning than a generic `Io` (e.g. a PCI device path that didn't
    /// exist at all vs. one that exists but is unreadable). First
    /// constructor lives in `source::sysfs::read_pci_device_description`,
    /// reached from `cmd::devicequery::run`.
    #[error("not found: {0}")]
    NotFound(String),

    /// D-Bus call failed. Constructed by `source::dbus` via
    /// `?`-propagation from `zbus::Error`.
    #[error("dbus: {0}")]
    Dbus(#[from] zbus::Error),

    /// Subprocess invocation failed.
    ///
    /// Wraps the typed `ExecError` from `source::exec`. The `#[from]`
    /// impl enables `?`-propagation from `run_with_timeout`. `Timeout`
    /// is reachable through this variant — there is no separate
    /// `SourceError::Timeout`.
    #[error("subprocess: {0}")]
    Subprocess(#[from] ExecError),
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
        // `zbus::Error::Unsupported` is one of the few variants with no
        // payload of its own — keeps the Display assertion stable across
        // zbus versions.
        let err = SourceError::Dbus(zbus::Error::Unsupported);
        assert!(
            err.to_string().starts_with("dbus: "),
            "expected dbus prefix, got {err}",
        );
    }

    #[test]
    fn from_zbus_error_via_question_mark() {
        fn call() -> Result<(), SourceError> {
            // ? should convert zbus::Error → SourceError::Dbus via #[from].
            Err(zbus::Error::Unsupported)?;
            Ok(())
        }
        let err = call().expect_err("should fail");
        assert!(
            matches!(err, SourceError::Dbus(_)),
            "expected SourceError::Dbus, got {err:?}",
        );
    }

    #[test]
    fn subprocess_variant_displays_with_subprocess_prefix() {
        let err = SourceError::Subprocess(ExecError::NotFound("journalctl".into()));
        assert_eq!(err.to_string(), "subprocess: binary not found: journalctl");
    }

    #[test]
    fn subprocess_variant_wraps_timeout() {
        let err = SourceError::Subprocess(ExecError::Timeout {
            timeout: std::time::Duration::from_secs(5),
        });
        assert!(
            err.to_string().contains("timeout"),
            "expected timeout message, got {err}",
        );
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

    #[test]
    fn from_exec_error_via_question_mark() {
        fn run() -> Result<(), SourceError> {
            // ? should convert ExecError → SourceError::Subprocess via #[from].
            Err(ExecError::NotFound("pactl".into()))?;
            Ok(())
        }
        let err = run().expect_err("should fail");
        assert!(
            matches!(err, SourceError::Subprocess(ExecError::NotFound(_))),
            "expected SourceError::Subprocess(NotFound), got {err:?}",
        );
    }
}
