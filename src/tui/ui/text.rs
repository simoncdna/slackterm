use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone};
use ratatui::style::Style;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::config::Clock;

pub type Styled = (String, Style);

/// Word-wraps styled text to `width` columns. Newlines start a new row and
/// words longer than a row are split.
pub fn wrap(segments: &[Styled], width: usize) -> Vec<Vec<Styled>> {
    let width = width.max(1);
    let mut rows: Vec<Vec<Styled>> = vec![Vec::new()];
    let mut row_width = 0;

    for (text, style) in segments {
        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                rows.push(Vec::new());
                row_width = 0;
            }
            for token in tokens(line) {
                let token_width = token.width();
                let is_space = token.chars().all(char::is_whitespace);
                if row_width + token_width > width && row_width > 0 {
                    if is_space {
                        continue;
                    }
                    rows.push(Vec::new());
                    row_width = 0;
                }
                if token_width <= width {
                    push(rows.last_mut().unwrap(), token, *style);
                    row_width += token_width;
                    continue;
                }
                for c in token.chars() {
                    let c_width = c.width().unwrap_or(0);
                    if row_width + c_width > width {
                        rows.push(Vec::new());
                        row_width = 0;
                    }
                    push(rows.last_mut().unwrap(), c.encode_utf8(&mut [0; 4]), *style);
                    row_width += c_width;
                }
            }
        }
    }
    rows
}

/// Splits into alternating runs of whitespace and non-whitespace.
fn tokens(line: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut previous: Option<bool> = None;
    for (offset, c) in line.char_indices() {
        let space = c.is_whitespace();
        if previous.is_some_and(|p| p != space) {
            tokens.push(&line[start..offset]);
            start = offset;
        }
        previous = Some(space);
    }
    if start < line.len() {
        tokens.push(&line[start..]);
    }
    tokens
}

fn push(row: &mut Vec<Styled>, text: &str, style: Style) {
    match row.last_mut() {
        Some((last, last_style)) if *last_style == style => last.push_str(text),
        _ => row.push((text.to_string(), style)),
    }
}

/// Cuts `text` to `width` columns, ending with `…` when shortened.
pub fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let c_width = c.width().unwrap_or(0);
        if used + c_width + 1 > width {
            break;
        }
        out.push(c);
        used += c_width;
    }
    out.push('…');
    out
}

/// `0:19`, `12:05`, `1:02:03`.
pub fn format_duration(milliseconds: u64) -> String {
    let seconds = milliseconds / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// `512 B`, `680 KB`, `10.7 MB`, `1.2 GB`.
pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1000.0;
    let bytes_f = bytes as f64;
    let (value, unit) = if bytes_f < KB {
        return format!("{bytes} B");
    } else if bytes_f < KB * KB {
        return format!("{} KB", (bytes_f / KB).round());
    } else if bytes_f < KB * KB * KB {
        (bytes_f / KB / KB, "MB")
    } else {
        (bytes_f / KB / KB / KB, "GB")
    };
    format!("{value:.1} {unit}")
}

/// Left-aligns `text` in `width` columns, cutting it if needed.
pub fn align_left(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let c_width = c.width().unwrap_or(0);
        if used + c_width > width {
            break;
        }
        out.push(c);
        used += c_width;
    }
    out + &" ".repeat(width - used)
}

/// Right-aligns `text` in `width` columns, truncating it if needed.
pub fn align_right(text: &str, width: usize) -> String {
    let text = truncate(text, width);
    format!("{}{text}", " ".repeat(width.saturating_sub(text.width())))
}

pub fn local_time(epoch_seconds: i64) -> DateTime<Local> {
    Local
        .timestamp_opt(epoch_seconds, 0)
        .single()
        .unwrap_or_else(Local::now)
}

const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        return "Today".to_string();
    }
    if today.pred_opt() == Some(day) {
        return "Yesterday".to_string();
    }
    let weekday = WEEKDAYS[day.weekday().num_days_from_monday() as usize];
    let month = MONTHS[day.month0() as usize];
    if day.year() == today.year() {
        format!("{weekday}, {month} {}", day.day())
    } else {
        format!("{weekday}, {month} {}, {}", day.day(), day.year())
    }
}

const SHORT_MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `14:02`, or `2:02 PM` on a 12-hour clock.
pub fn time_of_day(time: DateTime<Local>, clock: Clock) -> String {
    match clock {
        Clock::TwentyFourHour => time.format("%H:%M").to_string(),
        Clock::TwelveHour => time.format("%-I:%M %p").to_string(),
    }
}

/// The widest a `time_of_day` gets, to line times up in a column.
pub fn time_width(clock: Clock) -> usize {
    match clock {
        Clock::TwentyFourHour => 5,
        Clock::TwelveHour => 8,
    }
}

/// `10:02` today, `Oct 8 10:02` this year, `Oct 8, 2025` before.
pub fn short_datetime(time: DateTime<Local>, today: NaiveDate, clock: Clock) -> String {
    let day = time.date_naive();
    let month = SHORT_MONTHS[day.month0() as usize];
    if day == today {
        time_of_day(time, clock)
    } else if day.year() == today.year() {
        format!("{month} {} {}", day.day(), time_of_day(time, clock))
    } else {
        format!("{month} {}, {}", day.day(), day.year())
    }
}

const SHORT_WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// How long ago a conversation was active: `14:02`, `yesterday`, `Mon`,
/// `Oct 8`, then `Dec 2025`.
pub fn recency_label(time: DateTime<Local>, today: NaiveDate, clock: Clock) -> String {
    let day = time.date_naive();
    let days_ago = (today - day).num_days();
    let month = SHORT_MONTHS[day.month0() as usize];
    match days_ago {
        ..=0 => time_of_day(time, clock),
        1 => "yesterday".to_string(),
        2..=6 => SHORT_WEEKDAYS[day.weekday().num_days_from_monday() as usize].to_string(),
        _ if day.year() == today.year() => format!("{month} {}", day.day()),
        _ => format!("{month} {}", day.year()),
    }
}

/// Cuts styled text to `width` columns, ending with `…` when shortened.
pub fn truncate_styled(segments: Vec<Styled>, width: usize) -> Vec<Styled> {
    let mut out = Vec::new();
    let mut used = 0;
    for (text, style) in segments {
        let text_width = text.width();
        if used + text_width <= width {
            used += text_width;
            out.push((text, style));
            continue;
        }
        out.push((truncate(&text, width - used), style));
        break;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(rows: &[Vec<Styled>]) -> Vec<String> {
        rows.iter()
            .map(|row| row.iter().map(|(t, _)| t.as_str()).collect())
            .collect()
    }

    #[test]
    fn wraps_on_words() {
        let rows = wrap(&[("the deployment went out".into(), Style::new())], 12);
        assert_eq!(plain(&rows), ["the ", "deployment ", "went out"]);
    }

    #[test]
    fn splits_words_longer_than_a_row() {
        let rows = wrap(&[("abcdefgh".into(), Style::new())], 3);
        assert_eq!(plain(&rows), ["abc", "def", "gh"]);
    }

    #[test]
    fn keeps_newlines_and_styles() {
        let bold = Style::new().bold();
        let rows = wrap(&[("a\nb ".into(), Style::new()), ("c".into(), bold)], 10);
        assert_eq!(plain(&rows), ["a", "b c"]);
        assert_eq!(rows[1][1].1, bold);
    }

    #[test]
    fn truncates_with_an_ellipsis() {
        assert_eq!(truncate("deploys", 5), "depl…");
        assert_eq!(truncate("dev", 5), "dev");
        assert_eq!(align_right("ab", 4), "  ab");
        assert_eq!(align_left("ab", 4), "ab  ");
        assert_eq!(align_left("abcdef", 2), "ab");
    }

    #[test]
    fn formats_short_dates() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let at = |y, m, d| Local.with_ymd_and_hms(y, m, d, 10, 2, 0).single().unwrap();
        let clock = Clock::TwentyFourHour;
        assert_eq!(short_datetime(at(2026, 10, 8), today, clock), "10:02");
        assert_eq!(short_datetime(at(2026, 3, 2), today, clock), "Mar 2 10:02");
        assert_eq!(short_datetime(at(2025, 12, 1), today, clock), "Dec 1, 2025");
        let clock = Clock::TwelveHour;
        assert_eq!(
            short_datetime(at(2026, 3, 2), today, clock),
            "Mar 2 10:02 AM"
        );
    }

    #[test]
    fn writes_times_on_either_clock() {
        let at = |h, m| {
            Local
                .with_ymd_and_hms(2026, 10, 8, h, m, 0)
                .single()
                .unwrap()
        };
        assert_eq!(time_of_day(at(14, 2), Clock::TwentyFourHour), "14:02");
        assert_eq!(time_of_day(at(14, 2), Clock::TwelveHour), "2:02 PM");
        assert_eq!(time_of_day(at(0, 5), Clock::TwelveHour), "12:05 AM");
        assert_eq!(time_of_day(at(12, 59), Clock::TwelveHour).len(), 8);
        assert_eq!(time_width(Clock::TwelveHour), 8);
    }

    #[test]
    fn labels_recency() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let at = |y, m, d| Local.with_ymd_and_hms(y, m, d, 14, 2, 0).single().unwrap();
        let label = |y, m, d| recency_label(at(y, m, d), today, Clock::TwentyFourHour);
        assert_eq!(label(2026, 10, 8), "14:02");
        assert_eq!(label(2026, 10, 7), "yesterday");
        assert_eq!(label(2026, 10, 5), "Mon");
        assert_eq!(label(2026, 9, 2), "Sep 2");
        assert_eq!(label(2025, 12, 1), "Dec 2025");
        let label = recency_label(at(2026, 10, 8), today, Clock::TwelveHour);
        assert_eq!(label, "2:02 PM");
    }

    #[test]
    fn formats_durations_and_sizes() {
        assert_eq!(format_duration(19_686), "0:19");
        assert_eq!(format_duration(725_000), "12:05");
        assert_eq!(format_duration(3_723_000), "1:02:03");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(680_400), "680 KB");
        assert_eq!(format_size(10_747_897), "10.7 MB");
        assert_eq!(format_size(1_234_000_000), "1.2 GB");
    }

    #[test]
    fn truncates_styled_text() {
        let bold = Style::new().bold();
        let cut = truncate_styled(
            vec![("abc".into(), Style::new()), ("defgh".into(), bold)],
            6,
        );
        assert_eq!(
            cut,
            vec![("abc".into(), Style::new()), ("de…".into(), bold)]
        );
    }

    #[test]
    fn labels_days() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        assert_eq!(day_label(today, today), "Today");
        assert_eq!(day_label(today.pred_opt().unwrap(), today), "Yesterday");
        let monday = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        assert_eq!(day_label(monday, today), "Monday, October 5");
        let older = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();
        assert_eq!(day_label(older, today), "Wednesday, December 31, 2025");
    }
}
