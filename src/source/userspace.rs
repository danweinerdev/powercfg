//! Userspace tool integration via the `source::exec` helper.
//!
//! Hosts shellouts to tools that don't have a stable D-Bus API or
//! library equivalent for our purposes — currently `pactl` for audio
//! stream queries. Phase 4 adds `dmesg` and `journalctl`.

use std::process::Command;
use std::time::Duration;

use crate::model::requests::AudioStream;
use crate::source::SourceError;
use crate::source::exec::run_with_timeout;

/// Query PulseAudio/PipeWire for active sink-inputs (audio streams).
///
/// Runs `pactl list sink-inputs short` with a 5-second timeout. The
/// `?` propagates any `ExecError` (binary not found, timeout, generic
/// I/O) up as `SourceError::Subprocess(_)`. On a system without
/// `pactl` installed (e.g., a minimal container), the caller in
/// `cmd::requests` should `.unwrap_or_default()` this so the report
/// renders "no streams" instead of failing.
///
/// The output of `pactl list sink-inputs short` is tab-separated rows
/// of `<id>\t<sink>\t<client>\t<sample-spec>\t<volume>\t<mute>`. Only
/// `id` and `client` survive into the model.
pub fn list_audio_streams() -> Result<Vec<AudioStream>, SourceError> {
    let mut cmd = Command::new("pactl");
    cmd.args(["list", "sink-inputs", "short"]);
    let output = run_with_timeout(cmd, Duration::from_secs(5))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_pactl_short(&stdout))
}

/// Parse the tab-separated output of `pactl list sink-inputs short`.
///
/// One sink-input per line; columns are
/// `<id>\t<sink>\t<client>\t<sample-spec>\t<volume>\t<mute>`. We only
/// consume `id` (column 0) and `client` (column 2). Lines with fewer
/// than two columns are skipped — matches the Python guard
/// `if len(parts) >= 2:` so a malformed/short row doesn't poison the
/// rest of the report. A missing client column (column 2 absent on a
/// 2-column row) yields `"Unknown"`, matching Python line 137.
fn parse_pactl_short(stdout: &str) -> Vec<AudioStream> {
    stdout
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 2 {
                return None;
            }
            let id = parts[0].trim().to_owned();
            let client = parts
                .get(2)
                .map(|s| s.trim())
                .unwrap_or("Unknown")
                .to_owned();
            Some(AudioStream { id, client })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two-row output captured from a typical desktop session — Firefox
    /// and Spotify both holding sink-inputs.
    const FIREFOX_AND_SPOTIFY: &str = "\
123\talsa_output.pci-0000_00_1f.3.analog-stereo\tFirefox\ts16le 2ch 48000Hz\t65536 / 100% / 0.00 dB\tno
124\talsa_output.pci-0000_00_1f.3.analog-stereo\tspotify\ts16le 2ch 48000Hz\t52428 / 80% / -1.94 dB\tno
";

    #[test]
    fn parse_pactl_short_two_rows_yields_two_entries() {
        let streams = parse_pactl_short(FIREFOX_AND_SPOTIFY);
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].id, "123");
        assert_eq!(streams[0].client, "Firefox");
        assert_eq!(streams[1].id, "124");
        assert_eq!(streams[1].client, "spotify");
    }

    #[test]
    fn parse_pactl_short_empty_input_yields_empty_vec() {
        assert!(parse_pactl_short("").is_empty());
    }

    #[test]
    fn parse_pactl_short_skips_blank_lines() {
        let stdout = "\n\n123\tsink\tFirefox\trest\n\n";
        let streams = parse_pactl_short(stdout);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].id, "123");
        assert_eq!(streams[0].client, "Firefox");
    }

    #[test]
    fn parse_pactl_short_skips_rows_with_fewer_than_two_columns() {
        // Single-column row — matches Python's `if len(parts) >= 2:`
        // guard. The well-formed row before it must still be returned.
        let stdout = "123\tsink\tFirefox\trest\nlonely-id\n";
        let streams = parse_pactl_short(stdout);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].id, "123");
    }

    #[test]
    fn parse_pactl_short_two_column_row_falls_back_to_unknown_client() {
        // A 2-column row passes the `>= 2` guard but has no client
        // column at index 2 — Python falls back to "Unknown" and so do we.
        let stdout = "55\tsome-sink\n";
        let streams = parse_pactl_short(stdout);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].id, "55");
        assert_eq!(streams[0].client, "Unknown");
    }

    #[test]
    fn parse_pactl_short_tolerates_surrounding_whitespace() {
        // Some pactl variants emit trailing spaces; trimming each
        // column keeps the model clean.
        let stdout = "  123\tsink\tFirefox  \trest\n";
        let streams = parse_pactl_short(stdout);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].id, "123");
        assert_eq!(streams[0].client, "Firefox");
    }

    #[test]
    fn parse_pactl_short_space_separated_row_is_silently_skipped() {
        // PipeWire's pactl-compat layer has historically emitted
        // space-separated columns on some distro/version combinations.
        // The parser is strictly tab-separated, so a space-only row
        // produces a single "column" and is silently dropped — pin
        // that degradation behavior so a future "fix" doesn't end up
        // partially parsing the wrong column.
        let stdout = "123 sink Firefox rest\n";
        assert!(parse_pactl_short(stdout).is_empty());
    }

    /// Live integration test: hits the actual pactl binary.
    /// `#[ignore]` because CI containers may not have PulseAudio /
    /// PipeWire available; run with `cargo test -- --ignored` on the
    /// dev machine.
    #[test]
    #[ignore = "requires pactl and a running PulseAudio/PipeWire daemon; run with --ignored"]
    fn list_audio_streams_against_live_daemon() {
        let result = list_audio_streams();
        assert!(
            result.is_ok(),
            "list_audio_streams should succeed against live daemon: {:?}",
            result.err(),
        );
    }
}
