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
        let mut out = String::from("elapsed_ms,kind,user_id,display_name\n");
        for row in &self.rows {
            // Writing to a String cannot fail.
            let _ = writeln!(
                out,
                "{},{},{},{}",
                row.elapsed.as_millis(),
                row.kind.as_str(),
                csv_field(&row.user_id),
                csv_field(&row.display_name)
            );
        }
        out
    }
}

/// Quotes a CSV field when it holds a comma, quote or line break (RFC 4180). A field that a
/// spreadsheet would read as a formula (`=`, `+`, `-`, `@`, tab) gets a leading `'`, so opening the
/// log in Excel never runs anything a chatter typed into their name.
fn csv_field(value: &str) -> String {
    let value = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
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
    fn kinds_follow_outcome_and_match() {
        use MessageOutcome::*;
        assert_eq!(LogKind::from_outcome(Entered, true), LogKind::Entry);
        assert_eq!(LogKind::from_outcome(AlreadyEntered, true), LogKind::Repeat);
        assert_eq!(LogKind::from_outcome(AlreadyEntered, false), LogKind::Chat);
        assert_eq!(LogKind::from_outcome(Ignored, false), LogKind::Chat);
        assert_eq!(LogKind::from_outcome(Ignored, true), LogKind::LeftOut);
    }
}
