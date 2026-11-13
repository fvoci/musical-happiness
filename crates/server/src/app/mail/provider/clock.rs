//! The time of a signed request, in the form AWS reads: basic ISO 8601
//! (`20150830T123600Z`).

use std::time::{SystemTime, UNIX_EPOCH};

/// A moment, in whole seconds since the Unix epoch, broken into its UTC date and time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Moment {
    year: i64,
    month: u32,
    day: u32,
    hour: u64,
    minute: u64,
    second: u64,
}

impl Moment {
    /// Now, by the system's clock; a clock before 1970 reads as the epoch.
    pub(super) fn now() -> Self {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        Self::at(seconds)
    }

    /// The moment `seconds` after the Unix epoch.
    pub(super) fn at(seconds: u64) -> Self {
        let millis = i64::try_from(seconds)
            .unwrap_or(i64::MAX)
            .saturating_mul(1000);
        let (year, month, day) = crate::app::files::civil_from_millis(millis);
        let of_day = seconds % 86_400;
        Self {
            year,
            month,
            day,
            hour: of_day / 3600,
            minute: of_day % 3600 / 60,
            second: of_day % 60,
        }
    }

    /// `20150830T123600Z`, as `X-Amz-Date` gives it.
    pub(super) fn amz_date(self) -> String {
        format!(
            "{}T{:02}{:02}{:02}Z",
            self.amz_day(),
            self.hour,
            self.minute,
            self.second
        )
    }

    /// `20150830`, the day of a signature's scope.
    pub(super) fn amz_day(self) -> String {
        format!("{:04}{:02}{:02}", self.year, self.month, self.day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_reads_as_the_date_of_the_published_examples() {
        // The time of AWS's signature test suite: 2015-08-30T12:36:00Z.
        let aws = Moment::at(1_440_938_160);
        assert_eq!(aws.amz_date(), "20150830T123600Z");
        assert_eq!(aws.amz_day(), "20150830");
        assert_eq!(Moment::at(0).amz_date(), "19700101T000000Z");
        // A leap day, and the end of a year.
        assert_eq!(Moment::at(1_709_208_000).amz_date(), "20240229T120000Z");
        assert_eq!(Moment::at(1_735_689_599).amz_date(), "20241231T235959Z");
    }
}
