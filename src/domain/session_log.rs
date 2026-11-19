use std::fmt::Write as _;
use std::time::{Duration, Instant};

use super::giveaway::MessageOutcome;

/// What a chat message meant for the giveaway, as written to the CSV log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    /// First matching message: a new entrant.
    Entry,
    /// Matching message from someone already in.
    Repeat,
    /// Message without the keyword.
    Chat,
    /// Matching message from someone who could not enter (condition, weight 0, past winner).
    LeftOut,
    /// A drawn winner, appended to the log after entries closed.
    Winner,
}

impl LogKind {
    pub fn from_outcome(outcome: MessageOutcome, matched: bool) -> Self {
        match outcome {
            MessageOutcome::Entered => LogKind::Entry,
            MessageOutcome::AlreadyEntered if matched => LogKind::Repeat,
            MessageOutcome::AlreadyEntered | MessageOutcome::Ignored if !matched => LogKind::Chat,
            MessageOutcome::AlreadyEntered | MessageOutcome::Ignored => LogKind::LeftOut,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            LogKind::Entry => "entry",
            LogKind::Repeat => "repeat",
            LogKind::Chat => "chat",
            LogKind::LeftOut => "left_out",
            LogKind::Winner => "winner",
        }
    }
}

#[derive(Debug, Clone)]
struct Row {
    elapsed: Duration,
    kind: LogKind,
    user_id: String,
    display_name: String,
}

/// Every chat message seen while entries were open, for measuring the giveaway afterwards.
/// Message text is not kept.
#[derive(Debug, Clone)]
pub struct SessionLog {
    opened_at: Instant,
    rows: Vec<Row>,
}

impl SessionLog {
    pub fn new(opened_at: Instant) -> Self {
        Self {
            opened_at,
            rows: Vec::new(),
        }
    }

    /// When entries opened; rows count their time from here.
    pub fn opened_at(&self) -> Instant {
        self.opened_at
    }

    pub fn record(&mut self, now: Instant, kind: LogKind, user_id: &str, display_name: &str) {
        self.rows.push(Row {
            elapsed: now.saturating_duration_since(self.opened_at),
            kind,
            user_id: user_id.to_owned(),
            display_name: display_name.to_owned(),
        });
    }

    /// `elapsed_ms,kind,user_id,display_name`, one row per message.
    pub fn to_csv(&self) -> String {
        let mut out = String::from(HEADER);
        for row in &self.rows {
            out.push_str(&csv_row(
                row.elapsed,
                row.kind,
                &row.user_id,
                &row.display_name,
            ));
        }
        out
    }
}

/// One log line, newline included.
pub fn csv_row(elapsed: Duration, kind: LogKind, user_id: &str, display_name: &str) -> String {
    let mut line = String::new();
    // Writing to a String cannot fail.
    let _ = writeln!(
        line,
        "{},{},{},{}",
        elapsed.as_millis(),
        kind.as_str(),
        csv_field(user_id),
        csv_field(display_name)
    );
    line
}

/// What one giveaway log says, for the history page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    /// Chat messages seen while entries were open.
    pub messages: usize,
    pub entrants: usize,
    /// From opening entries to the last message seen before they closed.
    pub open_for: Duration,
    /// How long it took until 90 % of the entrants were in; `None` without entrants.
    pub ninety_percent_in: Option<Duration>,
    /// Drawn winners in draw order.
    pub winners: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SummaryError {
    #[error("not a giveaway log (unexpected header)")]
    Header,
    #[error("line {0} is not a valid log row")]
    Row(usize),
}

/// Reads a log written by [`SessionLog::to_csv`] and [`csv_row`].
pub fn summarize(csv: &str) -> Result<SessionSummary, SummaryError> {
    let mut lines = csv.lines();
    if lines.next() != Some(HEADER.trim_end()) {
        return Err(SummaryError::Header);
    }
    let mut messages = 0;
    let mut open_for = Duration::ZERO;
    let mut entry_times = Vec::new();
    let mut winners = Vec::new();
    for (index, line) in lines.enumerate().filter(|(_, line)| !line.is_empty()) {
        let line_number = index + 2;
        let fields = split_csv_line(line).ok_or(SummaryError::Row(line_number))?;
        let [elapsed, kind, _user_id, name] = fields.as_slice() else {
            return Err(SummaryError::Row(line_number));
        };
        let elapsed = elapsed
            .parse::<u64>()
            .map(Duration::from_millis)
            .map_err(|_| SummaryError::Row(line_number))?;
        match kind.as_str() {
            "winner" => winners.push(unneutralize(name)),
            "entry" | "repeat" | "chat" | "left_out" => {
                messages += 1;
                open_for = open_for.max(elapsed);
                if kind == "entry" {
                    entry_times.push(elapsed);
                }
            }
            _ => return Err(SummaryError::Row(line_number)),
        }
    }
    entry_times.sort();
    let ninety_percent_in =
        (!entry_times.is_empty()).then(|| entry_times[(entry_times.len() * 9).div_ceil(10) - 1]);
    Ok(SessionSummary {
        messages,
        entrants: entry_times.len(),
        open_for,
        ninety_percent_in,
        winners,
    })
}

const HEADER: &str = "elapsed_ms,kind,user_id,display_name\n";

/// Splits one RFC 4180 line; `None` when a quoted field never closes.
fn split_csv_line(line: &str) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => quoted = false,
            (false, '"') if field.is_empty() => quoted = true,
            (false, ',') => fields.push(std::mem::take(&mut field)),
            (_, c) => field.push(c),
        }
    }
    if quoted {
        return None;
    }
    fields.push(field);
    Some(fields)
}

/// Undoes the `'` that [`csv_field`] puts before formula-like names.
fn unneutralize(name: &str) -> String {
    match name.strip_prefix('\'') {
        Some(rest) if rest.starts_with(FORMULA_STARTS) => rest.to_owned(),
        _ => name.to_owned(),
    }
}

const FORMULA_STARTS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

/// Quotes a CSV field when it holds a comma, quote or line break (RFC 4180). A field that a
/// spreadsheet would read as a formula (`=`, `+`, `-`, `@`, tab) gets a leading `'`, so opening the
/// log in Excel never runs anything a chatter typed into their name.
fn csv_field(value: &str) -> String {
    let value = if value.starts_with(FORMULA_STARTS) {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_has_a_header_and_one_row_per_message() {
        let start = Instant::now();
        let mut log = SessionLog::new(start);
        log.record(
            start + Duration::from_millis(1500),
            LogKind::Entry,
            "7",
            "Lucky",
        );
        log.record(
            start + Duration::from_millis(2000),
            LogKind::Chat,
            "8",
            "Other",
        );
        assert_eq!(
            log.to_csv(),
            "elapsed_ms,kind,user_id,display_name\n1500,entry,7,Lucky\n2000,chat,8,Other\n"
        );
    }

    #[test]
    fn names_with_commas_or_quotes_are_quoted() {
        let start = Instant::now();
        let mut log = SessionLog::new(start);
        log.record(start, LogKind::Repeat, "1", "a,\"b\"");
        assert!(
            log.to_csv().ends_with("0,repeat,1,\"a,\"\"b\"\"\"\n"),
            "{}",
            log.to_csv()
        );
    }

    #[test]
    fn values_a_spreadsheet_would_run_as_formulas_are_neutralised() {
        let start = Instant::now();
        let mut log = SessionLog::new(start);
        log.record(start, LogKind::Chat, "1", "=HYPERLINK(\"x\")");
        log.record(start, LogKind::Chat, "2", "@cmd");
        let csv = log.to_csv();
        assert!(csv.contains("0,chat,1,\"'=HYPERLINK(\"\"x\"\")\""), "{csv}");
        assert!(csv.contains("0,chat,2,'@cmd"), "{csv}");
    }

    #[test]
    fn summary_counts_messages_entrants_and_winners() {
        let start = Instant::now();
        let mut log = SessionLog::new(start);
        for (ms, kind, id) in [
            (1_000, LogKind::Entry, "1"),
            (2_000, LogKind::Chat, "2"),
            (3_000, LogKind::Entry, "3"),
            (4_000, LogKind::Repeat, "1"),
            (9_000, LogKind::Entry, "4"),
        ] {
            log.record(start + Duration::from_millis(ms), kind, id, id);
        }
        let mut csv = log.to_csv();
        csv.push_str(&csv_row(
            Duration::from_secs(12),
            LogKind::Winner,
            "3",
            "=Lucky, \"the\" one",
        ));

        let summary = summarize(&csv).unwrap();

        assert_eq!(
            summary,
            SessionSummary {
                messages: 5,
                entrants: 3,
                open_for: Duration::from_secs(9),
                ninety_percent_in: Some(Duration::from_secs(9)),
                winners: vec!["=Lucky, \"the\" one".to_owned()],
            }
        );
    }

    #[test]
    fn ninety_percent_is_the_entry_that_completes_nine_tenths() {
        let start = Instant::now();
        let mut log = SessionLog::new(start);
        for s in 1..=20 {
            let id = s.to_string();
            log.record(start + Duration::from_secs(s), LogKind::Entry, &id, &id);
        }
        let summary = summarize(&log.to_csv()).unwrap();
        assert_eq!(summary.ninety_percent_in, Some(Duration::from_secs(18)));
    }

    #[test]
    fn a_log_without_entries_has_no_ninety_percent_time() {
        let summary = summarize("elapsed_ms,kind,user_id,display_name\n").unwrap();
        assert_eq!(summary.entrants, 0);
        assert_eq!(summary.ninety_percent_in, None);
    }

    #[test]
    fn foreign_files_and_broken_rows_are_errors() {
        assert_eq!(summarize("a,b\n"), Err(SummaryError::Header));
        assert_eq!(
            summarize("elapsed_ms,kind,user_id,display_name\n10,entry,1\n"),
            Err(SummaryError::Row(2))
        );
        assert_eq!(
            summarize("elapsed_ms,kind,user_id,display_name\n10,entry,1,\"open\n"),
            Err(SummaryError::Row(2))
        );
    }

    #[test]
    fn kinds_follow_outcome_and_match() {
        use MessageOutcome::*;
        assert_eq!(LogKind::from_outcome(Entered, true), LogKind::Entry);
        assert_eq!(LogKind::from_outcome(AlreadyEntered, true), LogKind::Repeat);
        assert_eq!(LogKind::from_outcome(AlreadyEntered, false), LogKind::Chat);
        assert_eq!(LogKind::from_outcome(Ignored, false), LogKind::Chat);
        assert_eq!(LogKind::from_outcome(Ignored, true), LogKind::LeftOut);
    }
}
