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
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Join a sysfs/procfs-relative path (e.g. `"sys/power/state"`) to the root.
    ///
    /// `rel` must not start with a `/` — leading slashes would replace the
    /// root path under `PathBuf::join` semantics, defeating the abstraction.
    pub fn join(&self, rel: impl AsRef<Path>) -> PathBuf {
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
    fn from_env_honors_powercfg_sysroot() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os(SYSROOT_ENV);
        // SAFETY: tests are serialized via ENV_LOCK; no other thread observes
        // the env while this test runs.
        unsafe { std::env::set_var(SYSROOT_ENV, "/tmp/fixture-root") };
        let root = SysRoot::from_env();
        assert_eq!(root.as_path(), Path::new("/tmp/fixture-root"));
        // Restore prior state so the next test gets a clean slate.
        unsafe {
            match prev {
                Some(v) => std::env::set_var(SYSROOT_ENV, v),
                None => std::env::remove_var(SYSROOT_ENV),
            }
        }
    }

    #[test]
    fn from_env_falls_back_to_default_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os(SYSROOT_ENV);
        // SAFETY: tests are serialized via ENV_LOCK.
        unsafe { std::env::remove_var(SYSROOT_ENV) };
        let root = SysRoot::from_env();
        assert_eq!(root.as_path(), Path::new("/"));
        unsafe {
            if let Some(v) = prev {
                std::env::set_var(SYSROOT_ENV, v);
            }
        }
    }
}
