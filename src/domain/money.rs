use std::fmt;

/// An amount in satoshis.
///
/// This is a value object: it has no identity, is immutable and copyable, and
/// its unit is part of the invariant. Two `Sats` values with the same number
/// are interchangeable. The type never uses floating point, so formatting and
/// arithmetic remain exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sats(u64);

impl Sats {
    pub const SATS_PER_BTC: u64 = 100_000_000;
    pub const ZERO: Self = Self(0);

    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Adds two amounts, returning `None` on overflow.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    /// Subtracts `other`, returning `None` when the result would be negative.
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    /// Compatibility alias for the canonical rendering.
    ///
    /// `Display` is the primary representation; this method remains for call
    /// sites that read more naturally with an explicit Bitcoin conversion.
    pub fn as_btc(self) -> String {
        self.to_string()
    }
}

/// Renders the amount in Bitcoin with exactly eight decimal places.
///
/// The arithmetic is intentionally integer-only: `f64` would introduce
/// precision and rounding concerns, especially above `2^53` sats.
impl fmt::Display for Sats {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}.{:08}",
            self.0 / Self::SATS_PER_BTC,
            self.0 % Self::SATS_PER_BTC
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values that exercise every boundary of the sat/BTC split, including
    /// the first integer an `f64` cannot represent exactly (`2^53 + 1`) and
    /// the maximum value accepted by `Sats`.
    const FORMAT_CASES: [(u64, &str); 16] = [
        (0, "0.00000000"),
        (1, "0.00000001"),
        (9, "0.00000009"),
        (10, "0.00000010"),
        (99, "0.00000099"),
        (100, "0.00000100"),
        (99_999_999, "0.99999999"),
        (100_000_000, "1.00000000"),
        (100_000_001, "1.00000001"),
        (550_000, "0.00550000"),
        (36_010_516_297, "360.10516297"),
        (15_464_503_162, "154.64503162"),
        (100_000_000_000, "1000.00000000"),
        (2_100_000_000_000_000, "21000000.00000000"),
        ((1u64 << 53) + 1, "90071992.54740993"),
        (u64::MAX, "184467440737.09551615"),
    ];

    #[test]
    fn formats_sats_as_bitcoin_with_exact_integer_arithmetic() {
        for (sats, expected) in FORMAT_CASES {
            assert_eq!(Sats::new(sats).to_string(), expected, "sats={sats}");
        }
    }

    #[test]
    fn formatted_amount_always_has_exactly_eight_decimal_places() {
        for (sats, _) in FORMAT_CASES {
            let formatted = Sats::new(sats).to_string();
            let (whole, fraction) = formatted.split_once('.').expect("decimal separator");

            assert!(!whole.is_empty(), "missing whole part: {formatted}");
            assert_eq!(fraction.len(), 8, "wrong precision: {formatted}");
            assert!(
                fraction.bytes().all(|byte| byte.is_ascii_digit()),
                "non-digit fraction: {formatted}"
            );
        }
    }

    #[test]
    fn formatted_amount_round_trips_to_the_original_sats() {
        let mut values: Vec<u64> = FORMAT_CASES.iter().map(|(sats, _)| *sats).collect();

        // Deterministic pseudo-random sample across the full `u64` range.
        // This avoids a dev-dependency while still exercising many values
        // that a hand-written table would never cover.
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..10_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            values.push(state);
        }

        for sats in values {
            let formatted = Sats::new(sats).to_string();
            assert_eq!(
                parse_bitcoin_amount(&formatted),
                sats,
                "formatted={formatted}"
            );
        }
    }

    #[test]
    fn stays_exact_beyond_the_f64_integer_limit() {
        // Every integer up to `2^53` is exact in `f64`; `2^53 + 1` is the
        // first one that is not. The expected rendering proves the conversion
        // never goes through a floating-point value.
        let sats = (1u64 << 53) + 1;

        assert_eq!(Sats::new(sats).to_string(), "90071992.54740993");
    }

    #[test]
    fn display_and_the_as_btc_alias_agree() {
        for (sats, expected) in FORMAT_CASES {
            assert_eq!(Sats::new(sats).as_btc(), expected);
            assert_eq!(Sats::new(sats).as_btc(), Sats::new(sats).to_string());
        }
    }

    #[test]
    fn zero_is_explicit_and_detected() {
        assert_eq!(Sats::ZERO, Sats::new(0));
        assert!(Sats::ZERO.is_zero());
        assert!(!Sats::new(1).is_zero());
    }

    #[test]
    fn checked_add_and_sub_guard_overflow() {
        assert_eq!(
            Sats::new(1_000).checked_add(Sats::new(500)),
            Some(Sats::new(1_500))
        );
        assert_eq!(
            Sats::new(1_000).checked_sub(Sats::new(400)),
            Some(Sats::new(600))
        );

        assert_eq!(Sats::new(u64::MAX).checked_add(Sats::new(1)), None);
        assert_eq!(Sats::new(0).checked_sub(Sats::new(1)), None);
    }

    #[test]
    fn orders_amounts_by_value() {
        assert!(Sats::new(1) < Sats::new(2));
        assert_eq!(Sats::new(3).cmp(&Sats::new(3)), std::cmp::Ordering::Equal);
    }

    /// Parses the API representation back into sats using only integer
    /// arithmetic. A successful round-trip proves no decimal digit was lost.
    fn parse_bitcoin_amount(formatted: &str) -> u64 {
        let (whole, fraction) = formatted.split_once('.').expect("decimal separator");
        assert_eq!(fraction.len(), 8, "wrong precision: {formatted}");

        let whole: u64 = whole.parse().expect("whole part");
        let fraction: u64 = fraction.parse().expect("fractional part");

        whole * Sats::SATS_PER_BTC + fraction
    }
}
