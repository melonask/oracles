use crate::error::{Error, Result};
use rust_decimal::Decimal;
use std::str::FromStr;

/// A strictly-positive rate amount backed by [`rust_decimal::Decimal`].
///
/// This type enforces that rates are always greater than zero and provides
/// helpers for parsing from strings, computing percent changes, and
/// displaying.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RateAmount(Decimal);

impl RateAmount {
    /// Parse a [`RateAmount`] from a string (e.g., `"3500.25"`).
    ///
    /// Returns an error if the string cannot be parsed as a decimal or if
    /// the resulting value is not positive.
    pub fn parse(input: &str) -> Result<Self> {
        let value =
            Decimal::from_str(input).map_err(|_| Error::InvalidDecimal(input.to_owned()))?;
        Self::from_decimal(value)
    }

    /// Construct a [`RateAmount`] from an existing [`Decimal`].
    ///
    /// Returns an error if the value is zero or negative.
    pub fn from_decimal(value: Decimal) -> Result<Self> {
        if value <= Decimal::ZERO {
            return Err(Error::InvalidRate(value.to_string()));
        }
        Ok(Self(value))
    }

    /// Return the inner [`Decimal`] value.
    pub fn decimal(&self) -> Decimal {
        self.0
    }

    /// Compute the absolute percent change between this rate and a previous one.
    ///
    /// Returns the change as a percentage (e.g., `"5.25"` for a 5.25% move).
    /// Returns an error if the previous rate is not positive.
    pub fn percent_change_from(&self, previous: &Self) -> Result<Decimal> {
        if previous.0 <= Decimal::ZERO {
            return Err(Error::InvalidRate(previous.0.to_string()));
        }
        let diff = (self.0 - previous.0).abs();
        Ok(diff / previous.0 * Decimal::new(100, 0))
    }
}

#[allow(missing_docs)]
impl core::fmt::Display for RateAmount {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}
