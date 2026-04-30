//! Filesystem root abstraction for sysfs/procfs reads.
//!
//! All source modules read paths via [`SysRoot`] so integration tests can
//! point the binary at a captured fixture tree instead of `/`.

use std::path::{Path, PathBuf};

/// Filesystem root for sysfs/procfs lookups.
///
/// Defaults to `/`. Tests construct one rooted at a `tempfile::tempdir()` so
/// parsers can be exercised against captured fixtures without root or specific
/// hardware.
#[derive(Debug, Clone)]
pub struct SysRoot(PathBuf);

/// Environment variable that overrides the default root.
///
/// Read by [`SysRoot::from_env`] and consumed by integration tests via
/// `assert_cmd::Command::env`.
pub const SYSROOT_ENV: &str = "POWERCFG_SYSROOT";

impl SysRoot {
    /// Construct a root at an explicit path.
    // TODO(phase-2): used by tests today; production callers in 2.x will
    // construct from explicit paths in a few CLI flag tests. Drop the
    // allow when one lands.
    #[allow(dead_code)]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// Read the root from `POWERCFG_SYSROOT`, falling back to `/`.
    pub fn from_env() -> Self {
        match std::env::var_os(SYSROOT_ENV) {
            Some(v) if !v.is_empty() => Self(PathBuf::from(v)),
            _ => Self::default(),
        }
    }

    /// Borrow the underlying root path.
    // TODO(phase-2): used by tests today; first production caller is
    // the device walker (2.x), which needs to read the raw root for
    // `read_dir` enumeration. Drop the allow when that lands.
    #[allow(dead_code)]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Join a sysfs/procfs-relative path (e.g. `"sys/power/state"`) to the root.
    ///
    /// `rel` must be relative. Absolute paths would silently replace the root
    /// under `PathBuf::join` semantics, defeating the abstraction and reading
    /// the live filesystem in tests — caught here with a debug assertion so
    /// the mistake fires loudly during `cargo test` instead of masking failures.
    pub fn join(&self, rel: impl AsRef<Path>) -> PathBuf {
        let rel = rel.as_ref();
        debug_assert!(
            rel.is_relative(),
            "SysRoot::join: rel must be relative, got {rel:?}",
        );
        self.0.join(rel)
    }
}

impl Default for SysRoot {
    fn default() -> Self {
        Self(PathBuf::from("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;

    // Tests in this module mutate the process environment, which is shared
    // across threads under `cargo test`. Serialize them so two tests cannot
    // race on `POWERCFG_SYSROOT`.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_root_is_filesystem_root() {
        let root = SysRoot::default();
        assert_eq!(root.as_path(), Path::new("/"));
    }

    #[test]
    fn join_under_default_root_produces_absolute_path() {
        let root = SysRoot::default();
        assert_eq!(
            root.join("sys/power/state"),
            PathBuf::from("/sys/power/state"),
        );
    }

    #[test]
    fn join_under_custom_root_stays_under_root() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let root = SysRoot::new(tmp.path());
        let joined = root.join("sys/power/state");
        assert!(
            joined.starts_with(tmp.path()),
            "{joined:?} should start with {:?}",
            tmp.path(),
        );
        assert!(joined.ends_with("sys/power/state"));
    }

    #[test]
    #[should_panic(expected = "rel must be relative")]
    fn join_panics_on_absolute_rel_in_debug() {
        // Absolute rel paths would silently replace the root in release; the
        // debug_assert keeps tests honest.
        let root = SysRoot::new("/tmp/fixture");
        let _ = root.join("/sys/power/state");
    }

    /// RAII guard that restores `POWERCFG_SYSROOT` to its prior value when
    /// dropped — guarantees cleanup even if the test panics on assertion
    /// failure, which a manual restore at the end of the test would miss.
    struct EnvGuard {
        prev: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn capture() -> Self {
            Self {
                prev: std::env::var_os(SYSROOT_ENV),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: tests holding an EnvGuard also hold ENV_LOCK, so no
            // other thread observes the env while this Drop runs.
            unsafe {
                match self.prev.take() {
                    Some(v) => std::env::set_var(SYSROOT_ENV, v),
                    None => std::env::remove_var(SYSROOT_ENV),
                }
            }
        }
    }

    #[test]
    fn from_env_honors_powercfg_sysroot() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = EnvGuard::capture();
        // SAFETY: serialized via ENV_LOCK.
        unsafe { std::env::set_var(SYSROOT_ENV, "/tmp/fixture-root") };
        let root = SysRoot::from_env();
        assert_eq!(root.as_path(), Path::new("/tmp/fixture-root"));
    }

    #[test]
    fn from_env_falls_back_to_default_when_unset() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = EnvGuard::capture();
        // SAFETY: serialized via ENV_LOCK.
        unsafe { std::env::remove_var(SYSROOT_ENV) };
        let root = SysRoot::from_env();
        assert_eq!(root.as_path(), Path::new("/"));
    }
}
