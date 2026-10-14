//! Pure, dependency-light formatting for amounts and timestamps this
//! explorer renders. Presentation only — never used for consensus-critical
//! arithmetic, and infallible (a malformed input degrades to a plain
//! fallback rather than panicking, since nothing here guards a safety
//! property).

/// Number of zatoshis in one ZEC.
const ZAT_PER_ZEC: i64 = 100_000_000;

/// Formats a zatoshi amount as a ZEC-denominated string, e.g.
/// `format_zec(100_000_000)` -> `"1.00000000 ZEC"`. The `i64` -> `f64`
/// conversion is exact for every amount this explorer renders (zatoshi
/// amounts stay far below `f64`'s 53-bit integer precision), so this is a
/// safe display-only cast, not a magnitude assumption about consensus
/// data.
pub fn format_zec(zat: i64) -> String {
    let zec = zat as f64 / ZAT_PER_ZEC as f64;
    format!("{zec:.8} ZEC")
}

/// Formats a zatoshi amount's USD value at `price_per_zec`, e.g.
/// `format_usd(100_000_000, 42.0)` -> `"$42.00"`.
pub fn format_usd(zat: i64, price_per_zec: f64) -> String {
    let zec = zat as f64 / ZAT_PER_ZEC as f64;
    format!("${:.2}", zec * price_per_zec)
}

/// Inserts thousands separators into an integer's decimal rendering, e.g.
/// `3514209` -> `"3,514,209"`. Pure string manipulation, no locale
/// awareness — this explorer has one audience.
pub fn group_thousands(value: impl std::fmt::Display) -> String {
    let rendered = value.to_string();
    let (sign, digits) = match rendered.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", rendered.as_str()),
    };
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{sign}{grouped}")
}

/// Shortens a long hex string for a list/table row: first 8 + "…" + last
/// 8 characters. The full value is never lost — callers keep it in an
/// `href`/title alongside this, and a detail page shows the hash in full
/// rather than calling this at all.
pub fn truncate_hash(hex: &str) -> String {
    const PREFIX_LEN: usize = 8;
    const SUFFIX_LEN: usize = 8;
    if hex.len() <= PREFIX_LEN + SUFFIX_LEN + 1 {
        return hex.to_string();
    }
    format!("{}…{}", &hex[..PREFIX_LEN], &hex[hex.len() - SUFFIX_LEN..])
}

/// Formats a Unix timestamp (seconds) as a human-readable UTC datetime,
/// e.g. `1700000300` -> `"2023-11-14 22:31:40 UTC"`. Falls back to the
/// raw number for a timestamp outside `time`'s representable range
/// (never happens for a real chain timestamp, but this is display code,
/// not a safety property, so a cosmetic fallback beats a panic).
pub fn format_timestamp(unix_secs: i64) -> String {
    match time::OffsetDateTime::from_unix_timestamp(unix_secs) {
        Ok(dt) => format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
            dt.year(),
            u8::from(dt.month()),
            dt.day(),
            dt.hour(),
            dt.minute(),
            dt.second()
        ),
        Err(_) => unix_secs.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_zec_renders_eight_decimal_places() {
        assert_eq!(format_zec(100_000_000), "1.00000000 ZEC");
        assert_eq!(format_zec(1), "0.00000001 ZEC");
        assert_eq!(format_zec(-1_000), "-0.00001000 ZEC");
    }

    #[test]
    fn format_usd_multiplies_by_the_given_price() {
        assert_eq!(format_usd(100_000_000, 42.0), "$42.00");
        assert_eq!(format_usd(50_000_000, 42.0), "$21.00");
    }

    #[test]
    fn group_thousands_inserts_separators_from_the_right() {
        assert_eq!(group_thousands(291), "291");
        assert_eq!(group_thousands(1_000), "1,000");
        assert_eq!(group_thousands(3_514_209u64), "3,514,209");
        assert_eq!(group_thousands(-1_000), "-1,000");
    }

    #[test]
    fn truncate_hash_leaves_short_values_alone_and_shortens_long_ones() {
        assert_eq!(truncate_hash("deadbeef"), "deadbeef");
        let long = "ab".repeat(32);
        let short = truncate_hash(&long);
        assert!(short.len() < long.len());
        assert!(short.starts_with("abababab"));
        assert!(short.ends_with("abababab"));
    }

    #[test]
    fn format_timestamp_renders_a_human_readable_utc_datetime() {
        assert_eq!(format_timestamp(1_700_000_300), "2023-11-14 22:18:20 UTC");
    }
}
