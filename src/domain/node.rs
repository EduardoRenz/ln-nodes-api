use chrono::{DateTime, SecondsFormat, Utc};

use crate::domain::error::DomainError;
use crate::domain::money::Sats;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey(String);

impl PublicKey {
    pub fn parse(raw: impl Into<String>) -> Result<Self, DomainError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(DomainError::EmptyPublicKey);
        }
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstSeen(DateTime<Utc>);

impl FirstSeen {
    pub fn from_unix(seconds: i64) -> Result<Self, DomainError> {
        DateTime::from_timestamp(seconds, 0)
            .map(Self)
            .ok_or(DomainError::InvalidTimestamp(seconds))
    }

    pub fn as_datetime(&self) -> DateTime<Utc> {
        self.0
    }

    pub fn to_rfc3339(&self) -> String {
        self.0.to_rfc3339_opts(SecondsFormat::Secs, true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub public_key: PublicKey,
    pub alias: String,
    pub capacity: Sats,
    pub first_seen: FirstSeen,
    pub rank_position: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_first_seen_as_utc_rfc3339() {
        let first_seen = FirstSeen::from_unix(1_522_941_222).expect("valid timestamp");
        assert_eq!(first_seen.to_rfc3339(), "2018-04-05T15:13:42Z");
    }

    #[test]
    fn rejects_empty_public_key() {
        assert_eq!(PublicKey::parse(""), Err(DomainError::EmptyPublicKey));
    }
}
