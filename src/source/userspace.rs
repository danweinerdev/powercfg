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
/// Runs `pactl list sink-inputs` (verbose form — no `short`) with a
/// 5-second timeout. The `?` propagates any `ExecError` (binary not
/// found, timeout, generic I/O) up as `SourceError::Subprocess(_)`. On
/// a system without `pactl` installed (e.g., a minimal container), the
/// caller in `cmd::requests` should `.unwrap_or_default()` this so the
/// report renders "no streams" instead of failing.
///
/// The verbose form returns `Sink Input #N` blocks containing a
/// property dictionary; we extract `application.name`,
/// `application.process.id`, and `application.process.binary` so the
/// printer can render `Client: <name> (<binary>, PID: <pid>)` instead
/// of the raw client *index* the `short` form exposed.
pub fn list_audio_streams() -> Result<Vec<AudioStream>, SourceError> {
    let mut cmd = Command::new("pactl");
    cmd.args(["list", "sink-inputs"]); // verbose form (no `short`)
    let output = run_with_timeout(cmd, Duration::from_secs(5))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_pactl_verbose(&stdout))
}

/// Parse the verbose `pactl list sink-inputs` output.
///
/// The format is a sequence of `Sink Input #N` blocks. Each block
/// contains a header line followed by tab-indented `Key: value` pairs
/// and a `Properties:` block of further-indented `key = "value"`
/// pairs. We walk the blocks, extract the ID from the header, and pick
/// out `application.name`, `application.process.id`, and
/// `application.process.binary` from the Properties block. Streams
/// without any of those properties have `Option`-None for the
/// corresponding fields and are still emitted (the printer renders
/// them as `Client: <unknown>`).
///
/// Indent depth varies between PulseAudio (`\t`) and PipeWire's
/// pactl-compat (spaces). We trim leading whitespace before testing
/// for `key = "value"` shape rather than committing to a specific
/// indent level. Multi-line quoted values aren't supported — if the
/// open quote and close quote land on different lines, the property
/// is silently dropped (the close quote on a later line just looks
/// like another malformed entry).
fn parse_pactl_verbose(stdout: &str) -> Vec<AudioStream> {
    let mut streams = Vec::new();
    let mut current: Option<PartialStream> = None;
    let mut in_properties = false;

    for line in stdout.lines() {
        // A new `Sink Input #N` header always starts a new block.
        // Whatever block we were assembling gets finalized first.
        if let Some(id) = parse_sink_input_header(line) {
            if let Some(stream) = current.take() {
                streams.push(stream.into_audio_stream());
            }
            current = Some(PartialStream::new(id));
            in_properties = false;
            continue;
        }

        let Some(stream) = current.as_mut() else {
            // Pre-block content (the `pactl` banner or stray output).
            continue;
        };

        let trimmed = line.trim_start();

        // The `Properties:` line itself starts the property region.
        // Match on the trimmed form so the indent kind (tab vs spaces)
        // doesn't matter.
        if trimmed == "Properties:" {
            in_properties = true;
            continue;
        }

        if !in_properties {
            // Top-level block fields like `Driver:`, `Owner Module:`,
            // `Client:` (the raw index — not what we want) etc. We
            // don't extract anything from them.
            continue;
        }

        // Inside the Properties block. Each entry is `key = "value"`.
        // If we hit a line that doesn't match (e.g. blank or a stray
        // top-level field that follows the block in some pactl
        // variants), we just skip it — the next `Sink Input #` header
        // will close out this block.
        if let Some((key, value)) = parse_property_line(trimmed) {
            stream.set_property(key, value);
        }
    }

    if let Some(stream) = current.take() {
        streams.push(stream.into_audio_stream());
    }

    streams
}

/// Builder accumulator for one `Sink Input #` block. Properties land
/// here as we encounter them; missing keys leave the field at its
/// default (`None`), which is exactly the graceful-degradation case
/// the printer handles.
struct PartialStream {
    id: String,
    application_name: Option<String>,
    pid: Option<u32>,
    binary: Option<String>,
}

impl PartialStream {
    fn new(id: String) -> Self {
        Self {
            id,
            application_name: None,
            pid: None,
            binary: None,
        }
    }

    fn set_property(&mut self, key: &str, value: &str) {
        match key {
            "application.name" => self.application_name = Some(value.to_owned()),
            "application.process.id" => {
                // Unparseable PID → keep `None`. Don't bubble an error
                // up; the printer falls back to a name-only render.
                self.pid = value.parse().ok();
            }
            "application.process.binary" => self.binary = Some(value.to_owned()),
            _ => {}
        }
    }

    fn into_audio_stream(self) -> AudioStream {
        AudioStream {
            id: self.id,
            application_name: self.application_name,
            pid: self.pid,
            binary: self.binary,
        }
    }
}

/// `Sink Input #449` → `Some("449")`. Anything else → `None`. The
/// header line is column-zero-anchored in pactl's output but we trim
/// for symmetry with the rest of the parser.
fn parse_sink_input_header(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("Sink Input #")?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(rest.to_owned())
}

/// `application.name = "Firefox"` → `Some(("application.name", "Firefox"))`.
///
/// Splits on the first ` = "` rather than `=` alone — the value can
/// contain `=` (e.g. `format.sample_format = "\"float32le\""`) but
/// the key never does. Returns `None` if the close quote can't be
/// found on the same line; multi-line values are silently dropped per
/// the parser docstring.
fn parse_property_line(line: &str) -> Option<(&str, &str)> {
    let (key, after) = line.split_once(" = \"")?;
    let value = after.strip_suffix('"')?;
    Some((key.trim(), value))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two-block sample captured from a typical desktop session —
    /// Firefox + Spotify both holding sink-inputs with full property
    /// metadata. Tabs preserved (PulseAudio's native indent style).
    const FIREFOX_AND_SPOTIFY_VERBOSE: &str = "\
Sink Input #449
\tDriver: protocol-native.c
\tOwner Module: 18
\tClient: 176
\tSink: 50
\tSample Specification: float32le 2ch 48000Hz
\tChannel Map: front-left,front-right
\tCorked: no
\tMute: no
\tProperties:
\t\tmedia.name = \"AudioStream\"
\t\tapplication.name = \"Firefox\"
\t\tapplication.process.id = \"12345\"
\t\tapplication.process.user = \"alice\"
\t\tapplication.process.binary = \"firefox\"
\t\tapplication.icon_name = \"firefox\"
\t\tmedia.role = \"video\"
Sink Input #7781
\tDriver: protocol-native.c
\tOwner Module: 18
\tClient: 7775
\tSink: 50
\tSample Specification: float32le 2ch 48000Hz
\tCorked: no
\tMute: no
\tProperties:
\t\tmedia.name = \"Playback\"
\t\tapplication.name = \"spotify\"
\t\tapplication.process.id = \"7775\"
\t\tapplication.process.binary = \"spotify\"
";

    #[test]
    fn parse_pactl_verbose_two_blocks_yields_two_entries() {
        let streams = parse_pactl_verbose(FIREFOX_AND_SPOTIFY_VERBOSE);
        assert_eq!(streams.len(), 2);

        let firefox = &streams[0];
        assert_eq!(firefox.id, "449");
        assert_eq!(firefox.application_name.as_deref(), Some("Firefox"));
        assert_eq!(firefox.pid, Some(12345));
        assert_eq!(firefox.binary.as_deref(), Some("firefox"));

        let spotify = &streams[1];
        assert_eq!(spotify.id, "7781");
        assert_eq!(spotify.application_name.as_deref(), Some("spotify"));
        assert_eq!(spotify.pid, Some(7775));
        assert_eq!(spotify.binary.as_deref(), Some("spotify"));
    }

    #[test]
    fn parse_pactl_verbose_missing_pid_property() {
        // application.name + binary present, no application.process.id.
        let stdout = "\
Sink Input #1
\tDriver: protocol-native.c
\tProperties:
\t\tapplication.name = \"Firefox\"
\t\tapplication.process.binary = \"firefox\"
";
        let streams = parse_pactl_verbose(stdout);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].id, "1");
        assert_eq!(streams[0].application_name.as_deref(), Some("Firefox"));
        assert_eq!(streams[0].pid, None);
        assert_eq!(streams[0].binary.as_deref(), Some("firefox"));
    }

    #[test]
    fn parse_pactl_verbose_missing_all_application_properties() {
        // Properties block exists but contains no application.* keys.
        // The entry must still be emitted — the printer renders it as
        // `Client: <unknown>` and the summary still counts it.
        let stdout = "\
Sink Input #99
\tDriver: protocol-native.c
\tProperties:
\t\tmedia.name = \"system\"
\t\tmedia.role = \"event\"
";
        let streams = parse_pactl_verbose(stdout);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].id, "99");
        assert_eq!(streams[0].application_name, None);
        assert_eq!(streams[0].pid, None);
        assert_eq!(streams[0].binary, None);
    }

    #[test]
    fn parse_pactl_verbose_no_properties_block() {
        // Block has no `Properties:` line at all (rare but possible
        // for stub streams). All Options None; entry still emitted.
        let stdout = "\
Sink Input #42
\tDriver: protocol-native.c
\tClient: 5
\tSink: 50
";
        let streams = parse_pactl_verbose(stdout);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].id, "42");
        assert_eq!(streams[0].application_name, None);
        assert_eq!(streams[0].pid, None);
        assert_eq!(streams[0].binary, None);
    }

    #[test]
    fn parse_pactl_verbose_empty_input() {
        assert!(parse_pactl_verbose("").is_empty());
    }

    #[test]
    fn parse_pactl_verbose_handles_single_block() {
        // End-of-input boundary: no following `Sink Input #` header
        // to trigger finalization. The trailing flush after the loop
        // must catch this case.
        let stdout = "\
Sink Input #7
\tProperties:
\t\tapplication.name = \"OnlyOne\"
\t\tapplication.process.id = \"42\"
\t\tapplication.process.binary = \"only\"
";
        let streams = parse_pactl_verbose(stdout);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].id, "7");
        assert_eq!(streams[0].application_name.as_deref(), Some("OnlyOne"));
        assert_eq!(streams[0].pid, Some(42));
        assert_eq!(streams[0].binary.as_deref(), Some("only"));
    }

    #[test]
    fn parse_pactl_verbose_unparseable_pid() {
        // PID column carries garbage. Rather than failing the whole
        // stream, drop pid → None and keep the rest.
        let stdout = "\
Sink Input #3
\tProperties:
\t\tapplication.name = \"Firefox\"
\t\tapplication.process.id = \"not-a-number\"
\t\tapplication.process.binary = \"firefox\"
";
        let streams = parse_pactl_verbose(stdout);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].application_name.as_deref(), Some("Firefox"));
        assert_eq!(streams[0].pid, None);
        assert_eq!(streams[0].binary.as_deref(), Some("firefox"));
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
