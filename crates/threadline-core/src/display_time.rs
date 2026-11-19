use chrono::{DateTime, NaiveDate, TimeZone};
use chrono_tz::{GapInfo, Tz};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayZone(Tz);

impl DisplayZone {
    pub const UTC: Self = Self(Tz::UTC);

    pub fn named(name: &str) -> Result<Self, &'static str> {
        name.parse::<Tz>()
            .map(Self)
            .map_err(|_| "Enter a valid IANA time zone, such as America/New_York.")
    }

    pub fn name(self) -> &'static str {
        self.0.name()
    }

    pub fn label(self, utc_ms: Option<i64>, missing: &str) -> String {
        utc_ms
            .and_then(DateTime::from_timestamp_millis)
            .map_or_else(
                || missing.to_owned(),
                |instant| {
                    instant
                        .with_timezone(&self.0)
                        .format("%Y-%m-%d %H:%M %Z (%:z)")
                        .to_string()
                },
            )
    }

    /// UTC bounds of a local calendar day. A time zone can skip midnight or even a whole date.
    pub fn day_bounds(self, text: &str) -> Option<(i64, i64)> {
        let date = NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?;
        let midnight = |day: NaiveDate| {
            let naive = day.and_hms_opt(0, 0, 0)?;
            self.0
                .from_local_datetime(&naive)
                .earliest()
                .or_else(|| GapInfo::new(&naive, &self.0)?.end)
                .map(|at| at.timestamp_millis())
        };
        let start = midnight(date)?;
        let end = midnight(date.succ_opt()?)?;
        (start < end).then_some((start, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(s: &str) -> i64 {
        DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
    }

    #[test]
    fn display_timezone_preserves_instants_across_new_york_dst() {
        let zone = DisplayZone::named("America/New_York").unwrap();
        assert_eq!(
            zone.label(Some(ms("2026-11-01T01:30:00-04:00")), ""),
            "2026-11-01 01:30 EDT (-04:00)"
        );
        assert_eq!(
            zone.label(Some(ms("2026-11-01T01:30:00-05:00")), ""),
            "2026-11-01 01:30 EST (-05:00)"
        );
        assert_eq!(
            zone.day_bounds("2026-03-08"),
            Some((ms("2026-03-08T05:00:00Z"), ms("2026-03-09T04:00:00Z")))
        );
        assert_eq!(
            zone.day_bounds("2026-11-01"),
            Some((ms("2026-11-01T04:00:00Z"), ms("2026-11-02T05:00:00Z")))
        );
        assert_eq!(zone.label(None, "unknown"), "unknown");
        assert_eq!(zone.day_bounds("2026-02-30"), None);
        assert!(DisplayZone::named("Local/fake").is_err());
        let apia = DisplayZone::named("Pacific/Apia").unwrap();
        assert_eq!(apia.day_bounds("2011-12-30"), None);
        assert_eq!(
            apia.day_bounds("2011-12-29").unwrap().1,
            apia.day_bounds("2011-12-31").unwrap().0
        );
        let before = |day| zone.day_bounds(day).unwrap().0;
        assert_eq!(before("2026-03-09") - before("2026-03-08"), 23 * 3_600_000);
        assert_eq!(before("2026-11-02") - before("2026-11-01"), 25 * 3_600_000);
    }
}
