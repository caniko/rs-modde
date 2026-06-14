pub fn format_timestamp(secs: i64) -> String {
    use std::fmt::Write;
    let dt = time_to_parts(secs);
    let mut s = String::new();
    let _ = write!(
        s,
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        dt.0, dt.1, dt.2, dt.3, dt.4, dt.5
    );
    s
}

/// Format a Unix timestamp as a short date: `"Apr 12 14:30"`.
#[must_use]
pub fn format_timestamp_short(secs: i64) -> String {
    let (y, m, d, hour, minute, _) = time_to_parts(secs);
    let month = match m {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    };

    // Include year if it differs from current year (approximate: 2026)
    let current_year = {
        let now_days = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 86400) as i32;
        let z = now_days + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = (z - era * 146097) as u32;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        yoe as i32 + era * 400
    };

    if y == current_year {
        format!("{month} {d} {hour:02}:{minute:02}")
    } else {
        format!("{month} {d} '{:02} {hour:02}:{minute:02}", y % 100)
    }
}

/// Break a Unix timestamp into `(year, month, day, hour, minute, second)`.
#[must_use]
pub fn time_to_parts(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i32;
    let time_of_day = (secs % 86400) as u32;
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;

    // Civil date from days since 1970-01-01 (Euclidean algorithm)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i32 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    (y, m, d, hour, minute, second)
}
