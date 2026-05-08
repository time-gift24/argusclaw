use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::{errors::CronError, Cron};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScheduledMessageError {
    #[error("invalid cron expression: {0}")]
    InvalidCron(String),

    #[error("invalid timezone: {0}")]
    InvalidTimezone(String),

    #[error("cron expression has no next run")]
    NoNextRun,
}

pub fn next_cron_run(
    expr: &str,
    timezone: Option<&str>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, ScheduledMessageError> {
    let cron = Cron::from_str(expr).map_err(invalid_cron)?;
    let timezone = parse_timezone(timezone)?;
    let local_now = now.with_timezone(&timezone);

    cron.find_next_occurrence(&local_now, false)
        .map(|next| next.with_timezone(&Utc))
        .map_err(next_run_error)
}

fn parse_timezone(timezone: Option<&str>) -> Result<Tz, ScheduledMessageError> {
    match timezone.map(str::trim).filter(|timezone| !timezone.is_empty()) {
        Some(timezone) => timezone
            .parse()
            .map_err(|_| ScheduledMessageError::InvalidTimezone(timezone.to_owned())),
        None => Ok(Tz::UTC),
    }
}

fn invalid_cron(error: CronError) -> ScheduledMessageError {
    ScheduledMessageError::InvalidCron(error.to_string())
}

fn next_run_error(error: CronError) -> ScheduledMessageError {
    match error {
        CronError::TimeSearchLimitExceeded => ScheduledMessageError::NoNextRun,
        error => ScheduledMessageError::InvalidCron(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{next_cron_run, ScheduledMessageError};
    use chrono::{DateTime, Utc};

    #[test]
    fn next_cron_run_respects_timezone() {
        let now = parse_utc("2026-05-07T00:00:00Z");

        let next = next_cron_run("0 9 * * *", Some("Asia/Shanghai"), now).unwrap();

        assert_eq!(next, parse_utc("2026-05-07T01:00:00Z"));
    }

    #[test]
    fn next_cron_run_rejects_invalid_cron() {
        let now = parse_utc("2026-05-07T00:00:00Z");

        let error = next_cron_run("not a cron", Some("Asia/Shanghai"), now).unwrap_err();

        assert!(matches!(error, ScheduledMessageError::InvalidCron(_)));
    }

    #[test]
    fn next_cron_run_rejects_invalid_timezone() {
        let now = parse_utc("2026-05-07T00:00:00Z");

        let error = next_cron_run("0 9 * * *", Some("Not/AZone"), now).unwrap_err();

        assert!(matches!(error, ScheduledMessageError::InvalidTimezone(_)));
    }

    #[test]
    fn blank_timezone_defaults_to_utc() {
        let now = parse_utc("2026-05-07T00:00:00Z");

        let next = next_cron_run("0 9 * * *", Some("   "), now).unwrap();

        assert_eq!(next, parse_utc("2026-05-07T09:00:00Z"));
    }

    fn parse_utc(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }
}
