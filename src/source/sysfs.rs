//! sysfs readers for `/sys`.
//!
//! Functions take a `&SysRoot` so integration tests can point at fixture
//! trees (`POWERCFG_SYSROOT=tests/fixtures/sys-typical`). Every reader
//! returns `Result<T, SourceError>`; the command layer decides whether a
//! missing file or unreadable permission should render as "no data" or as
//! a hard error.

use std::fs;

use crate::paths::SysRoot;
use crate::source::SourceError;

/// Read `/sys/power/state` — a space-separated list of the kernel-supported
/// sleep state tokens (e.g. `freeze mem disk`).
///
/// An empty file yields `Ok(vec![])`. I/O failures (missing file,
/// permission denied) propagate as `SourceError::Io`.
pub fn read_sleep_states(root: &SysRoot) -> Result<Vec<String>, SourceError> {
    let path = root.join("sys/power/state");
    let content = fs::read_to_string(&path)?;
    Ok(content.split_whitespace().map(|s| s.to_owned()).collect())
}

/// Read `/sys/power/mem_sleep`, parsing the bracketed-current syntax.
///
/// The kernel writes this file as a space-separated list of supported
/// memory-sleep modes with the currently selected one wrapped in square
/// brackets, e.g. `[s2idle] deep`. The bracket marker is stripped from
/// the mode name when added to the modes list (matching Python's
/// `get_mem_sleep_modes`); the bare value is returned as the second tuple
/// element.
///
/// Returns `(modes, current)` where `current` is `None` if no bracketed
/// token was present.
pub fn read_mem_sleep_modes(root: &SysRoot) -> Result<(Vec<String>, Option<String>), SourceError> {
    let path = root.join("sys/power/mem_sleep");
    let content = fs::read_to_string(&path)?;
    Ok(parse_bracketed_modes(&content))
}

/// Read `/sys/power/disk` (hibernation mode), parsing the bracketed-current
/// syntax. Same shape as [`read_mem_sleep_modes`].
pub fn read_disk_modes(root: &SysRoot) -> Result<(Vec<String>, Option<String>), SourceError> {
    let path = root.join("sys/power/disk");
    let content = fs::read_to_string(&path)?;
    Ok(parse_bracketed_modes(&content))
}

/// Read `/sys/power/image_size` — the maximum hibernation image size in
/// bytes as a single decimal integer.
///
/// Missing file → `SourceError::Io` (the caller maps this to "section
/// absent"). A non-numeric or empty value → `SourceError::Parse` so the
/// failure is distinguishable in tests.
pub fn read_image_size_bytes(root: &SysRoot) -> Result<u64, SourceError> {
    let path = root.join("sys/power/image_size");
    let content = fs::read_to_string(&path)?;
    let trimmed = content.trim();
    trimmed
        .parse::<u64>()
        .map_err(|e| SourceError::Parse(format!("image_size: {e} (input: {trimmed:?})")))
}

/// Parse the kernel's bracketed-current syntax into `(modes, current)`.
///
/// Mirrors Python `get_mem_sleep_modes`/`get_disk_modes`: the bracketed
/// token is treated as both a member of the modes list (with brackets
/// stripped) and the current marker. Whitespace-only or empty input
/// returns `(vec![], None)`. Unmatched/partial brackets fall back to the
/// raw token, matching the Python `startswith("[") and endswith("]")`
/// guard.
fn parse_bracketed_modes(s: &str) -> (Vec<String>, Option<String>) {
    let mut modes = Vec::new();
    let mut current = None;
    for tok in s.split_whitespace() {
        if tok.starts_with('[') && tok.ends_with(']') && tok.len() >= 2 {
            let inner = &tok[1..tok.len() - 1];
            current = Some(inner.to_owned());
            modes.push(inner.to_owned());
        } else {
            modes.push(tok.to_owned());
        }
    }
    (modes, current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_fixture(root: &SysRoot, rel: &str, content: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).expect("create dirs");
        fs::write(&path, content).expect("write fixture");
    }

    #[test]
    fn parse_bracketed_modes_current_first() {
        let (modes, current) = parse_bracketed_modes("[s2idle] deep");
        assert_eq!(modes, vec!["s2idle".to_string(), "deep".to_string()]);
        assert_eq!(current.as_deref(), Some("s2idle"));
    }

    #[test]
    fn parse_bracketed_modes_current_last() {
        let (modes, current) = parse_bracketed_modes("s2idle [deep]");
        assert_eq!(modes, vec!["s2idle".to_string(), "deep".to_string()]);
        assert_eq!(current.as_deref(), Some("deep"));
    }

    #[test]
    fn parse_bracketed_modes_no_bracket_marker() {
        let (modes, current) = parse_bracketed_modes("s2idle");
        assert_eq!(modes, vec!["s2idle".to_string()]);
        assert!(current.is_none());
    }

    #[test]
    fn parse_bracketed_modes_empty() {
        let (modes, current) = parse_bracketed_modes("");
        assert!(modes.is_empty());
        assert!(current.is_none());
    }

    #[test]
    fn parse_bracketed_modes_extra_whitespace() {
        // Leading/trailing whitespace + a tab in the middle should still
        // tokenize cleanly via split_whitespace.
        let (modes, current) = parse_bracketed_modes("  [s2idle]   deep  ");
        assert_eq!(modes, vec!["s2idle".to_string(), "deep".to_string()]);
        assert_eq!(current.as_deref(), Some("s2idle"));
    }

    #[test]
    fn read_sleep_states_returns_tokens() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/power/state", "freeze mem disk\n");
        assert_eq!(
            read_sleep_states(&root).expect("read"),
            vec!["freeze", "mem", "disk"],
        );
    }

    #[test]
    fn read_sleep_states_empty_file_returns_empty_vec() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/power/state", "");
        assert!(read_sleep_states(&root).expect("read").is_empty());
    }

    #[test]
    fn read_sleep_states_missing_file_is_io_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let err = read_sleep_states(&root).expect_err("missing file should error");
        assert!(matches!(err, SourceError::Io(_)));
    }

    #[test]
    fn read_mem_sleep_modes_parses_bracketed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/power/mem_sleep", "[s2idle] deep\n");
        let (modes, current) = read_mem_sleep_modes(&root).expect("read");
        assert_eq!(modes, vec!["s2idle".to_string(), "deep".to_string()]);
        assert_eq!(current.as_deref(), Some("s2idle"));
    }

    #[test]
    fn read_disk_modes_parses_bracketed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/power/disk", "[platform] shutdown reboot\n");
        let (modes, current) = read_disk_modes(&root).expect("read");
        assert_eq!(modes, vec!["platform", "shutdown", "reboot"]);
        assert_eq!(current.as_deref(), Some("platform"));
    }

    #[test]
    fn read_image_size_bytes_parses_decimal() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/power/image_size", "4194304000\n");
        assert_eq!(read_image_size_bytes(&root).expect("read"), 4_194_304_000);
    }

    #[test]
    fn read_image_size_bytes_missing_is_io_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let err = read_image_size_bytes(&root).expect_err("missing file should error");
        assert!(matches!(err, SourceError::Io(_)));
    }

    #[test]
    fn read_image_size_bytes_non_numeric_is_parse_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/power/image_size", "not-a-number\n");
        let err = read_image_size_bytes(&root).expect_err("non-numeric should error");
        assert!(matches!(err, SourceError::Parse(_)));
    }
}
