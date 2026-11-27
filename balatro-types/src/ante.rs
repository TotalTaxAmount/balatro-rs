#![allow(non_upper_case_globals)]

use std::convert::TryFrom;
use std::fmt;

/// An ante level. Endless mode continues past ante 8, so this is an
/// unbounded number rather than a closed enum.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ante(u32);

impl Ante {
    pub const Zero: Self = Self(0);
    pub const One: Self = Self(1);
    pub const Two: Self = Self(2);
    pub const Three: Self = Self(3);
    pub const Four: Self = Self(4);
    pub const Five: Self = Self(5);
    pub const Six: Self = Self(6);
    pub const Seven: Self = Self(7);
    pub const Eight: Self = Self(8);

    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u32 {
        self.0
    }

    pub fn as_usize(self) -> usize {
        self.0 as usize
    }

    pub fn as_i32(self) -> i32 {
        i32::try_from(self.0).unwrap_or(i32::MAX)
    }

    /// Base chip requirement for the ante. Antes above 8 use Balatro's
    /// endless scaling formula (`get_blind_amount` on the White stake).
    pub fn base(self) -> usize {
        match self.0 {
            0 => 100,
            1 => 300,
            2 => 800,
            3 => 2_000,
            4 => 5_000,
            5 => 11_000,
            6 => 20_000,
            7 => 35_000,
            8 => 50_000,
            ante => endless_base(ante),
        }
    }

    pub fn next(self, max: Self) -> Option<Self> {
        if self.0 >= max.0 {
            return None;
        }
        self.0.checked_add(1).map(Self)
    }

    /// One ante lower, `None` at `Zero`. The Hieroglyph/Petroglyph
    /// vouchers ("-1 Ante") move a run backwards along this.
    pub fn prev(self) -> Option<Self> {
        self.0.checked_sub(1).map(Self)
    }

    fn name(self) -> Option<&'static str> {
        Some(match self.0 {
            0 => "Zero",
            1 => "One",
            2 => "Two",
            3 => "Three",
            4 => "Four",
            5 => "Five",
            6 => "Six",
            7 => "Seven",
            8 => "Eight",
            _ => return None,
        })
    }

    #[cfg(feature = "serde")]
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "Zero" => Self::Zero,
            "One" => Self::One,
            "Two" => Self::Two,
            "Three" => Self::Three,
            "Four" => Self::Four,
            "Five" => Self::Five,
            "Six" => Self::Six,
            "Seven" => Self::Seven,
            "Eight" => Self::Eight,
            _ => return None,
        })
    }
}

/// Port of `get_blind_amount` for antes above 8 with the standard White
/// stake scaling (`k = 0.75`, base 50,000). Values are truncated to the
/// leading significant digits exactly like the client.
fn endless_base(ante: u32) -> usize {
    let k = 0.75f64;
    let base = 50_000f64;
    let c = (ante - 8) as f64;
    let d = 1.0 + 0.2 * c;
    let amount = (base * (1.6 + (k * c).powf(d)).powf(c)).floor();
    if !amount.is_finite() {
        return usize::MAX;
    }
    let magnitude = 10f64.powi(amount.log10().floor() as i32 - 1);
    let amount = amount - amount % magnitude;
    amount as usize
}

impl fmt::Debug for Ante {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => formatter.write_str(name),
            None => write!(formatter, "Ante({})", self.0),
        }
    }
}

impl TryFrom<usize> for Ante {
    type Error = ();

    fn try_from(i: usize) -> Result<Self, Self::Error> {
        u32::try_from(i).map(Self).map_err(|_| ())
    }
}

impl From<u32> for Ante {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<u8> for Ante {
    fn from(value: u8) -> Self {
        Self(value.into())
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Ante {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.name() {
            Some(name) => serializer.serialize_str(name),
            None => serializer.serialize_u32(self.0),
        }
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Ante {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct AnteVisitor;

        impl serde::de::Visitor<'_> for AnteVisitor {
            type Value = Ante;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an ante name such as \"Eight\" or a non-negative integer")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                u32::try_from(value)
                    .map(Ante)
                    .map_err(|_| E::custom("ante is out of range"))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value < 0 {
                    return Err(E::custom("ante cannot be negative"));
                }
                self.visit_u64(value as u64)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ante::from_name(value)
                    .or_else(|| value.parse::<u32>().ok().map(Ante))
                    .ok_or_else(|| E::custom(format!("invalid ante {value}")))
            }
        }

        deserializer.deserialize_any(AnteVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ante_base() {
        assert_eq!(Ante::Zero.base(), 100);
        assert_eq!(Ante::Eight.base(), 50000);
    }

    #[test]
    fn test_endless_ante_base() {
        assert_eq!(Ante::new(9).base(), 110_000);
        assert_eq!(Ante::new(10).base(), 560_000);
        assert_eq!(Ante::new(11).base(), 7_200_000);
        assert_eq!(Ante::new(12).base(), 300_000_000);
        assert_eq!(Ante::new(13).base(), 47_000_000_000);
    }

    #[test]
    fn test_ante_next_stops_at_max() {
        assert_eq!(Ante::Zero.next(Ante::Eight), Some(Ante::One));
        assert_eq!(Ante::Eight.next(Ante::Eight), None);
        assert_eq!(Ante::Eight.next(Ante::new(12)), Some(Ante::new(9)));
        assert_eq!(Ante::new(12).next(Ante::new(12)), None);
    }

    #[test]
    fn test_ante_prev_and_try_from() {
        assert_eq!(Ante::Zero.prev(), None);
        assert_eq!(Ante::Eight.prev(), Some(Ante::Seven));
        assert_eq!(Ante::try_from(9usize), Ok(Ante::new(9)));
        assert_eq!(Ante::try_from(usize::MAX), Err(()));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_ante_serde_backwards_compatible() {
        assert_eq!(
            serde_json::from_str::<Ante>("\"Eight\"").unwrap(),
            Ante::Eight
        );
        assert_eq!(serde_json::from_str::<Ante>("9").unwrap(), Ante::new(9));
        assert_eq!(
            serde_json::from_str::<Ante>("\"11\"").unwrap(),
            Ante::new(11)
        );
        assert_eq!(serde_json::to_string(&Ante::Eight).unwrap(), "\"Eight\"");
        assert_eq!(serde_json::to_string(&Ante::new(9)).unwrap(), "9");
    }
}
