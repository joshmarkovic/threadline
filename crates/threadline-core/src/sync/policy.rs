use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Daily intervals begin on an enabled weekday. Monday is bit zero.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSchedule {
    pub weekdays: u8,
    pub start_minute: u16,
    pub end_minute: u16,
    /// None follows changes to the operating system's IANA time zone.
    pub timezone: Option<String>,
    pub work_interval_secs: u32,
    pub off_hours_interval_secs: u32,
    pub visible_interval_secs: u32,
}

impl Default for WorkSchedule {
    fn default() -> Self {
        Self {
            weekdays: 0b0011111,
            start_minute: 9 * 60,
            end_minute: 17 * 60,
            timezone: None,
            work_interval_secs: 120,
            off_hours_interval_secs: 1800,
            visible_interval_secs: 60,
        }
    }
}

impl WorkSchedule {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.weekdays > 127
            || self.start_minute >= 1440
            || self.end_minute > 1440
            || self.start_minute == self.end_minute
        {
            return Err("Invalid workdays or work window");
        }
        if [
            self.work_interval_secs,
            self.off_hours_interval_secs,
            self.visible_interval_secs,
        ]
        .contains(&0)
        {
            return Err("Refresh intervals must be positive");
        }
        if self
            .timezone
            .as_ref()
            .is_some_and(|tz| tz.parse::<Tz>().is_err())
        {
            return Err("Unknown IANA time zone");
        }
        Ok(())
    }

    pub fn in_work_window(&self, now_ms: i64, system_zone: &str) -> Result<bool, &'static str> {
        self.validate()?;
        let timezone = self
            .timezone
            .as_deref()
            .unwrap_or(system_zone)
            .parse::<Tz>()
            .map_err(|_| "Unknown system time zone")?;
        let local = DateTime::<Utc>::from_timestamp_millis(now_ms)
            .ok_or("Unsupported observation time")?
            .with_timezone(&timezone);
        let minute = (local.hour() * 60 + local.minute()) as u16;
        let weekday = local.weekday().num_days_from_monday();
        let enabled = |day| self.weekdays & (1u8 << day) != 0;
        if self.start_minute < self.end_minute {
            Ok(enabled(weekday) && (self.start_minute..self.end_minute).contains(&minute))
        } else {
            Ok((minute >= self.start_minute && enabled(weekday))
                || (minute < self.end_minute && enabled((weekday + 6) % 7)))
        }
    }

    pub fn interval(
        &self,
        now_ms: i64,
        system_zone: &str,
        priority: ReadPriority,
    ) -> Result<Duration, &'static str> {
        let work = self.in_work_window(now_ms, system_zone)?;
        let routine = if work {
            self.work_interval_secs
        } else {
            self.off_hours_interval_secs
        };
        let secs = match priority {
            ReadPriority::Visible => routine.min(self.visible_interval_secs),
            ReadPriority::Reconcile => routine.saturating_mul(4),
            _ => routine,
        };
        Ok(Duration::from_secs(secs.into()))
    }
}

pub fn system_timezone() -> Result<String, &'static str> {
    iana_time_zone::get_timezone().map_err(|_| "Operating system time zone unavailable")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SyncControls {
    pub work_offline: bool,
    pub pause_background: bool,
    pub pause_queued_actions: bool,
    pub notification_quiet: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ReadPriority {
    Explicit,
    Visible,
    Activity,
    Recent,
    Reconcile,
}

impl SyncControls {
    pub fn can_read(self, priority: ReadPriority) -> bool {
        !self.work_offline && (!self.pause_background || priority == ReadPriority::Explicit)
    }
    pub fn can_send(self) -> bool {
        !self.work_offline && !self.pause_queued_actions
    }
}

/// An absolute persisted retry floor and a monotonic in-process delay use distinct clocks.
pub fn retry_delay(failures: u32, server_minimum: Duration) -> Option<Duration> {
    const STEPS: [u64; 4] = [5, 15, 30, 60];
    let delay = STEPS.get(failures.checked_sub(1)? as usize)?;
    Some(Duration::from_secs(*delay).max(server_minimum))
}

/// Start from persisted provider update time, with overlap for interrupted pages and timestamp precision.
pub fn activity_since(last_update_ms: Option<i64>, walk_started_ms: i64) -> i64 {
    const OVERLAP_MS: i64 = 5 * 60_000;
    last_update_ms
        .unwrap_or(walk_started_ms)
        .saturating_sub(OVERLAP_MS)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn utc(iso: &str) -> i64 {
        iso.parse::<DateTime<Utc>>().unwrap().timestamp_millis()
    }

    #[test]
    fn ut_005_dst_gap_fold_overnight_and_system_zone_keep_local_workdays() {
        let mut schedule = WorkSchedule {
            weekdays: 1 << 6,
            start_minute: 60,
            end_minute: 150,
            timezone: Some("America/New_York".into()),
            ..Default::default()
        };
        let work = |s: &WorkSchedule, iso| s.in_work_window(utc(iso), "UTC").unwrap();
        assert!(work(&schedule, "2026-03-08T06:59:59Z"));
        assert!(!work(&schedule, "2026-03-08T07:00:00Z"));
        assert!(work(&schedule, "2026-11-01T05:30:00Z"));
        assert!(work(&schedule, "2026-11-01T06:30:00Z"));
        schedule.start_minute = 23 * 60;
        schedule.end_minute = 2 * 60;
        assert!(work(&schedule, "2026-11-02T06:00:00Z"));
        assert!(!work(&schedule, "2026-11-02T07:00:00Z"));
        let t = utc("2026-11-02T14:00:00Z");
        assert!(
            WorkSchedule::default()
                .in_work_window(t, "America/New_York")
                .unwrap()
        );
        assert!(
            !WorkSchedule::default()
                .in_work_window(t, "America/Los_Angeles")
                .unwrap()
        );
        assert!(retry_delay(1, Duration::from_secs(90)).unwrap() >= Duration::from_secs(90));
        assert!(retry_delay(5, Duration::ZERO).is_none());
    }

    #[test]
    fn ut_006_controls_keep_background_dispatch_and_delivery_independent() {
        let mut c = SyncControls {
            pause_background: true,
            notification_quiet: true,
            ..Default::default()
        };
        assert!(c.can_send());
        assert!(c.can_read(ReadPriority::Explicit));
        assert!(!c.can_read(ReadPriority::Visible));
        c.pause_queued_actions = true;
        assert!(!c.can_send());
        c.work_offline = true;
        assert!(!c.can_read(ReadPriority::Explicit));
    }
}
#[test]
fn ut_002_sparse_numbers_do_not_define_the_provider_update_window() {
    let old_issue_last_update = 3_000_000;
    let newer_number_seen = 3_400_000;
    let restarted = 4_000_000;
    assert_eq!(activity_since(None, restarted), 3_700_000);
    let since = activity_since(Some(newer_number_seen), restarted);
    assert!(old_issue_last_update < since);
    assert!(
        3_450_000 > since,
        "an edit to the old issue stays eligible after #900 was cached"
    );
    assert_eq!(since, 3_100_000);
    assert_eq!(activity_since(Some(i64::MIN), restarted), i64::MIN);
}

#[test]
fn ut_007_retry_and_server_floors_have_independent_bounds() {
    use crate::github::RateLimit;

    let rate = RateLimit {
        retry_after_seconds: Some(5),
        poll_after_seconds: Some(60),
        remaining: Some(0),
        reset_at_unix_seconds: Some(180),
        ..Default::default()
    };
    assert_eq!(rate.not_before_ms(100_000), Some(180_000));
    assert_eq!(
        RateLimit {
            remaining: Some(3),
            ..rate
        }
        .not_before_ms(100_000),
        Some(160_000)
    );
    assert_eq!(
        retry_delay(1, Duration::from_secs(80)),
        Some(Duration::from_secs(80))
    );
    assert_eq!(
        retry_delay(4, Duration::ZERO),
        Some(Duration::from_secs(60))
    );
    assert_eq!(retry_delay(5, Duration::ZERO), None);
}
