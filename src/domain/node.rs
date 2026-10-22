use chrono::{DateTime, SecondsFormat, Utc};

use crate::domain::error::DomainError;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sats(u64);

impl Sats {
    pub const SATS_PER_BTC: u64 = 100_000_000;

    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn as_btc(self) -> String {
        format!(
            "{}.{:08}",
            self.0 / Self::SATS_PER_BTC,
            self.0 % Self::SATS_PER_BTC
        )
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
    fn formats_capacity_as_bitcoin_with_eight_decimals() {
        assert_eq!(Sats::new(36_010_516_297).as_btc(), "360.10516297");
        assert_eq!(Sats::new(550_000).as_btc(), "0.00550000");
        assert_eq!(Sats::new(0).as_btc(), "0.00000000");
    }

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
