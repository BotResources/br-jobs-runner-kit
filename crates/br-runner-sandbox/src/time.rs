use std::fmt;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(DateTime<Utc>);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not an RFC 3339 time")]
pub struct InvalidTimestamp(String);

impl Timestamp {
    pub fn from_system_time(time: SystemTime) -> Self {
        let utc = DateTime::<Utc>::from(time);
        Self(
            Utc.timestamp_millis_opt(utc.timestamp_millis())
                .single()
                .unwrap_or(utc),
        )
    }

    pub fn now() -> Self {
        Self::from_system_time(SystemTime::now())
    }

    pub fn parse(text: &str) -> Result<Self, InvalidTimestamp> {
        DateTime::parse_from_rfc3339(text)
            .map(|time| Self::from_system_time(time.with_timezone(&Utc).into()))
            .map_err(|_| InvalidTimestamp(text.to_owned()))
    }

    pub fn to_system_time(self) -> SystemTime {
        self.0.into()
    }

    pub fn plus(self, span: Duration) -> Self {
        let span = chrono::Duration::from_std(span).unwrap_or(chrono::Duration::MAX);
        Self(
            self.0
                .checked_add_signed(span)
                .unwrap_or(DateTime::<Utc>::MAX_UTC),
        )
    }

    pub fn has_passed(self, now: Timestamp) -> bool {
        self.0 <= now.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.to_rfc3339_opts(SecondsFormat::Millis, true))
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}
