use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone};
use ratatui::style::Style;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
];
const MONTHS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];

pub fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        return "Aujourd'hui".to_string();
    }
    if today.pred_opt() == Some(day) {
        return "Hier".to_string();
    }
    let weekday = WEEKDAYS[day.weekday().num_days_from_monday() as usize];
    let month = MONTHS[day.month0() as usize];
    if day.year() == today.year() {
        format!("{weekday} {} {month}", day.day())
    } else {
        format!("{weekday} {} {month} {}", day.day(), day.year())
    }
}

const SHORT_MONTHS: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];

/// `10:02` today, `8 oct. 10:02` this year, `8 oct. 2025` before.
pub fn short_datetime(time: DateTime<Local>, today: NaiveDate) -> String {
    let day = time.date_naive();
    let month = SHORT_MONTHS[day.month0() as usize];
    if day == today {
        time.format("%H:%M").to_string()
    } else if day.year() == today.year() {
        format!("{} {month} {}", day.day(), time.format("%H:%M"))
    } else {
        format!("{} {month} {}", day.day(), day.year())
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
        let rows = wrap(&[("le déploiement est passé".into(), Style::new())], 12);
        assert_eq!(plain(&rows), ["le ", "déploiement ", "est passé"]);
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
    }

    #[test]
    fn formats_short_dates() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let at = |y, m, d| Local.with_ymd_and_hms(y, m, d, 10, 2, 0).single().unwrap();
        assert_eq!(short_datetime(at(2026, 10, 8), today), "10:02");
        assert_eq!(short_datetime(at(2026, 3, 2), today), "2 mars 10:02");
        assert_eq!(short_datetime(at(2025, 12, 1), today), "1 déc. 2025");
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
    fn labels_days_in_french() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        assert_eq!(day_label(today, today), "Aujourd'hui");
        assert_eq!(day_label(today.pred_opt().unwrap(), today), "Hier");
        let monday = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        assert_eq!(day_label(monday, today), "lundi 5 octobre");
        let older = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();
        assert_eq!(day_label(older, today), "mercredi 31 décembre 2025");
    }
}
