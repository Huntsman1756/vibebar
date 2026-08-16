use chrono::{Datelike, Local, NaiveDate, TimeZone, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRange {
    Today,
    Last7Days,
    Last30Days,
    ThisMonth,
}

pub fn local_day(timestamp_millis: i64) -> String {
    Utc.timestamp_millis_opt(timestamp_millis)
        .single()
        .expect("timestamp millis should be valid")
        .with_timezone(&Local)
        .date_naive()
        .format("%F")
        .to_string()
}

pub fn range_start(range: HistoryRange, today: NaiveDate) -> NaiveDate {
    match range {
        HistoryRange::Today => today,
        HistoryRange::Last7Days => today - chrono::Days::new(6),
        HistoryRange::Last30Days => today - chrono::Days::new(29),
        HistoryRange::ThisMonth => today.with_day(1).expect("valid first day of month"),
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::{HistoryRange, range_start};

    #[test]
    fn computes_deterministic_history_window_starts() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 16).unwrap();

        assert_eq!(range_start(HistoryRange::Today, today), today);
        assert_eq!(
            range_start(HistoryRange::Last7Days, today),
            NaiveDate::from_ymd_opt(2026, 8, 10).unwrap()
        );
        assert_eq!(
            range_start(HistoryRange::Last30Days, today),
            NaiveDate::from_ymd_opt(2026, 7, 18).unwrap()
        );
        assert_eq!(
            range_start(HistoryRange::ThisMonth, today),
            NaiveDate::from_ymd_opt(2026, 8, 1).unwrap()
        );
    }
}
