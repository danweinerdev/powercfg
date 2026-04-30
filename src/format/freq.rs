//! CPU frequency formatting.
//!
//! Mirrors the Python `format_freq` (`powercfg.py` lines 931-937):
//! ≥1_000_000 kHz renders as `"X.XX GHz"` (two decimals), ≥1_000 kHz
//! renders as `"X MHz"` (no decimals), otherwise `"X kHz"`.

/// Format a frequency in kHz as a human-readable string.
// TODO(phase-2): consumed by cmd::energy (CPU current/min/max). Drop the
// allow when 2.x lands.
#[allow(dead_code)]
pub fn format_freq(khz: u64) -> String {
    if khz >= 1_000_000 {
        // Python: f"{khz / 1000000:.2f} GHz" — two decimal places, half-even
        // rounding via Rust's default float formatter (matches CPython for
        // these inputs).
        format!("{:.2} GHz", khz as f64 / 1_000_000.0)
    } else if khz >= 1_000 {
        // Python: f"{khz / 1000:.0f} MHz" — no decimals, half-even rounding.
        format!("{:.0} MHz", khz as f64 / 1_000.0)
    } else {
        format!("{khz} kHz")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kilohertz_below_one_megahertz() {
        assert_eq!(format_freq(0), "0 kHz");
        assert_eq!(format_freq(500), "500 kHz");
        assert_eq!(format_freq(999), "999 kHz");
    }

    #[test]
    fn megahertz_boundary() {
        // 1000 kHz is exactly 1 MHz; Python prints "1 MHz".
        assert_eq!(format_freq(1_000), "1 MHz");
    }

    #[test]
    fn megahertz_typical() {
        assert_eq!(format_freq(400_000), "400 MHz");
        // 999_500..=999_999 kHz all round to "1000 MHz" under half-even
        // rounding — intentional Python parity, not a formatter bug.
        assert_eq!(format_freq(999_999), "1000 MHz");
    }

    #[test]
    fn gigahertz_boundary() {
        assert_eq!(format_freq(1_000_000), "1.00 GHz");
    }

    #[test]
    fn gigahertz_typical() {
        assert_eq!(format_freq(3_400_000), "3.40 GHz");
        assert_eq!(format_freq(4_800_000), "4.80 GHz");
    }
}
