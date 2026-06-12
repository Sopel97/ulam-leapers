use std::fmt::Display;
use std::ops::{Add, BitAnd, Div, Mul, Not, Shl, Shr, Sub};

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum Pow2Error {
    NotAPowerOf2,
}

/// Models a `u64` that is guaranteed to be a power of two.
/// The number is stored as an exponent ONLY, allowing for
/// some specific operations, like floored division, to be
/// performed much faster if `Pow2` is the divisor.
///
/// # Example usage
///
/// ```
/// use ulam_leapers::math::pow2::{div_floor, Pow2};
/// const ALIGNMENT: Pow2 = Pow2::from_exponent(5);
/// // Division uses bit shifts and is branchless
/// assert_eq!(div_floor(37, ALIGNMENT), 1);
/// assert_eq!(div_floor(-37, ALIGNMENT), -2);
/// ```
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Copy)]
pub struct Pow2 {
    exponent: u8,
}

impl Pow2 {
    /// Creates a `Pow2` representing `2^exponent`.
    /// If `exponent > 63`, that is, the number would not fit in `u64`,
    /// the function panics.
    pub const fn from_exponent(exponent: u8) -> Pow2 {
        if exponent > 63 {
            panic!("The provided exponent is too large");
        }

        Pow2 { exponent }
    }

    /// Returns the `exponent` of the modeled `2^exponent` number.
    pub fn exponent(&self) -> u8 {
        self.exponent
    }

    /// Returns the next power of two. If the next power of two would not fit
    /// in a `u64` the function panics.
    pub fn next(self) -> Pow2 {
        Pow2::from_exponent(self.exponent + 1)
    }

    /// Returns the modeled power of two number as a `u64`.
    /// This function always succeeds because the only exponents
    /// compatible with `u64` are allowed.
    pub fn as_u64(&self) -> u64 {
        1u64 << self.exponent
    }
}

impl Display for Pow2 {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.as_u64())
    }
}

impl TryFrom<u64> for Pow2 {
    type Error = Pow2Error;

    /// Attempts to convert a given `value` into a `Pow2`.
    /// If the given `value` is not a power two then
    /// `Err(Pow2Error::NotAPowerOf2)` is returned.
    /// Otherwise, a `Pow2` modeling a number equal to `value` is returned.
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value.is_power_of_two() {
            Ok(Pow2 {
                exponent: value.ilog2() as u8,
            })
        } else {
            Err(Pow2Error::NotAPowerOf2)
        }
    }
}

impl From<Pow2> for u64 {
    fn from(p: Pow2) -> Self {
        1u64 << p.exponent
    }
}

impl Mul for Pow2 {
    type Output = Pow2;
    fn mul(self, other: Pow2) -> Pow2 {
        Pow2::from_exponent(self.exponent + other.exponent)
    }
}

impl Div for Pow2 {
    type Output = Pow2;
    fn div(self, other: Pow2) -> Pow2 {
        if other.exponent >= self.exponent {
            Pow2::from_exponent(0)
        } else {
            Pow2::from_exponent(self.exponent - other.exponent)
        }
    }
}

/// Division rounding towards negative infinity.
/// Supports both signed and unsigned integers as input.
/// If exponent of `b` is higher than maximum bitshift allowed by `T`
/// the result is defined but incorrect.
pub fn div_floor<T>(a: T, b: Pow2) -> T
where
    T: Shr<Output = T> + From<u8>,
{
    a >> T::from(b.exponent)
}

/// Division rounding towards positive infinity.
/// Supports both signed and unsigned integers as input.
/// If exponent of `b` is higher than maximum bitshift allowed by `T`
/// the result is defined but incorrect.
pub fn div_ceil<T>(a: T, b: Pow2) -> T
where
    T: Copy + Shl<Output = T> + Shr<Output = T> + Add<Output = T> + Sub<Output = T> + From<u8>,
{
    let one = T::from(1u8);
    let mask = (one << T::from(b.exponent)) - one;

    (a + mask) >> T::from(b.exponent)
}

/// Returns `true` if `a` is a multiple of the number modeled by `b`.
/// Supports both signed and unsigned integers as input.
/// If exponent of `b` is higher than maximum bitshift allowed by `T`
/// the result is defined but incorrect.
pub fn is_multiple_of<T>(a: T, b: Pow2) -> bool
where
    T: Shr<Output = T> + Shl<Output = T> + From<u8> + Copy + Eq,
{
    floor_to_multiple(a, b) == a
}

/// Remainder of division rounding towards negative infinity.
/// The remainder is always positive.
/// Supports both signed and unsigned integers as input.
/// If exponent of `b` is higher than maximum bitshift allowed by `T`
/// the result is defined but incorrect.
pub fn mod_floor<T>(a: T, b: Pow2) -> T
where
    T: Shr<Output = T> + Sub<Output = T> + Shl<Output = T> + From<u8> + Copy,
{
    a - floor_to_multiple(a, b)
}

/// Rounds the given value `a` towards negative infinity to the
/// next multiple of the number modeled by `b`.
/// Supports both signed and unsigned integers as input.
/// If exponent of `b` is higher than maximum bitshift allowed by `T`
/// the result is defined but incorrect.
pub fn floor_to_multiple<T>(a: T, b: Pow2) -> T
where
    T: Shr<Output = T> + Shl<Output = T> + From<u8> + Copy,
{
    let exp = T::from(b.exponent);
    a >> exp << exp
}

/// Rounds the given value `a` towards positive infinity to the
/// next multiple of the number modeled by `b`.
/// Supports both signed and unsigned integers as input.
/// If exponent of `b` is higher than maximum bitshift allowed by `T`
/// the result is defined but incorrect.
pub fn ceil_to_multiple<T>(a: T, b: Pow2) -> T
where
    T: Copy
        + Shl<Output = T>
        + Add<Output = T>
        + Sub<Output = T>
        + BitAnd<Output = T>
        + Not<Output = T>
        + From<u8>,
{
    let one = T::from(1u8);
    let mask = (one << T::from(b.exponent)) - one;

    (a + mask) & !mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_exponent_zero_is_one() {
        let p = Pow2::from_exponent(0);
        assert_eq!(p.as_u64(), 1);
        assert_eq!(p.exponent(), 0);
    }

    #[test]
    fn from_exponent_max_valid() {
        let p = Pow2::from_exponent(63);
        assert_eq!(p.as_u64(), 1u64 << 63);
    }

    #[test]
    #[should_panic]
    fn from_exponent_64_panics() {
        Pow2::from_exponent(64);
    }

    #[test]
    fn try_from_u64_powers_of_two() {
        for exp in 0u8..=63 {
            let value: u64 = 1 << exp;
            let p = Pow2::try_from(value).expect("should succeed for power of two");
            assert_eq!(p.exponent(), exp);
            assert_eq!(p.as_u64(), value);
        }
    }

    #[test]
    fn try_from_u64_zero_is_not_a_power_of_two() {
        assert_eq!(Pow2::try_from(0u64), Err(Pow2Error::NotAPowerOf2));
    }

    #[test]
    fn try_from_u64_non_powers_fail() {
        for n in [3u64, 5, 6, 7, 9, 10, 100, 1023, u64::MAX] {
            assert_eq!(
                Pow2::try_from(n),
                Err(Pow2Error::NotAPowerOf2),
                "expected {n} to fail"
            );
        }
    }

    #[test]
    fn into_u64_roundtrip() {
        let p = Pow2::try_from(1024).unwrap();
        let v: u64 = p.into();
        assert_eq!(v, 1024);
        assert_eq!(p.as_u64(), 1024);
    }

    #[test]
    fn next_increments_exponent() {
        let p = Pow2::from_exponent(4);
        assert_eq!(p.next().exponent(), 5);
        assert_eq!(p.next().as_u64(), 32);
    }

    #[test]
    #[should_panic]
    fn next_on_max_exponent_panics() {
        Pow2::from_exponent(63).next();
    }

    #[test]
    fn mul_adds_exponents() {
        let a = Pow2::from_exponent(3);
        let b = Pow2::from_exponent(4);
        let c = a * b;
        assert_eq!(c.exponent(), 7);
        assert_eq!(c.as_u64(), 128);
    }

    #[test]
    fn mul_by_one_is_identity() {
        let one = Pow2::from_exponent(0);
        let p = Pow2::from_exponent(5);
        assert_eq!((p * one).exponent(), 5);
    }

    #[test]
    fn ordering_by_exponent() {
        let small = Pow2::from_exponent(2);
        let large = Pow2::from_exponent(10);
        assert!(small < large);
        assert!(large > small);
        assert_eq!(small, small);
    }

    #[test]
    fn display_shows_numeric_value() {
        assert_eq!(format!("{}", Pow2::from_exponent(0)), "1");
        assert_eq!(format!("{}", Pow2::from_exponent(10)), "1024");
    }

    #[test]
    fn div_floor_u64_exact() {
        assert_eq!(div_floor(32u64, Pow2::from_exponent(5)), 1);
        assert_eq!(div_floor(64u64, Pow2::from_exponent(3)), 8);
    }

    #[test]
    fn div_floor_u64_rounds_down() {
        assert_eq!(div_floor(37u64, Pow2::from_exponent(5)), 1);
        assert_eq!(div_floor(63u64, Pow2::from_exponent(5)), 1);
    }

    #[test]
    fn div_floor_u64_zero() {
        assert_eq!(div_floor(0u64, Pow2::from_exponent(5)), 0);
    }

    #[test]
    fn div_floor_i64_positive() {
        assert_eq!(div_floor(37i64, Pow2::from_exponent(5)), 1);
    }

    #[test]
    fn div_floor_i64_exact_negative() {
        assert_eq!(div_floor(-32i64, Pow2::from_exponent(5)), -1);
    }

    #[test]
    fn div_floor_i64_negative_rounds_toward_neg_inf() {
        assert_eq!(div_floor(-37i64, Pow2::from_exponent(5)), -2);
        assert_eq!(div_floor(-1i64, Pow2::from_exponent(5)), -1);
    }

    #[test]
    fn div_floor_by_one_is_identity() {
        assert_eq!(div_floor(42i64, Pow2::from_exponent(0)), 42);
        assert_eq!(div_floor(-42i64, Pow2::from_exponent(0)), -42);
    }

    #[test]
    fn div_ceil_u64_exact() {
        assert_eq!(div_ceil(32u64, Pow2::from_exponent(5)), 1);
    }

    #[test]
    fn div_ceil_u64_rounds_up() {
        assert_eq!(div_ceil(33u64, Pow2::from_exponent(5)), 2);
        assert_eq!(div_ceil(63u64, Pow2::from_exponent(5)), 2);
    }

    #[test]
    fn div_ceil_u64_zero() {
        assert_eq!(div_ceil(0u64, Pow2::from_exponent(5)), 0);
    }

    #[test]
    fn div_ceil_i64_negative() {
        assert_eq!(div_ceil(-37i64, Pow2::from_exponent(5)), -1);
        assert_eq!(div_ceil(-32i64, Pow2::from_exponent(5)), -1);
        assert_eq!(div_ceil(-1i64, Pow2::from_exponent(5)), 0);
        assert_eq!(div_ceil(-33i64, Pow2::from_exponent(5)), -1);
    }

    #[test]
    fn div_ceil_by_one_is_identity() {
        assert_eq!(div_ceil(42i64, Pow2::from_exponent(0)), 42);
        assert_eq!(div_ceil(-42i64, Pow2::from_exponent(0)), -42);
    }

    #[test]
    fn floor_to_multiple_u64_already_aligned() {
        assert_eq!(floor_to_multiple(64u64, Pow2::from_exponent(6)), 64);
    }

    #[test]
    fn floor_to_multiple_u64_unaligned() {
        assert_eq!(floor_to_multiple(65u64, Pow2::from_exponent(6)), 64);
        assert_eq!(floor_to_multiple(127u64, Pow2::from_exponent(6)), 64);
    }

    #[test]
    fn floor_to_multiple_i64_positive() {
        assert_eq!(floor_to_multiple(37i64, Pow2::from_exponent(5)), 32);
    }

    #[test]
    fn floor_to_multiple_i64_negative() {
        assert_eq!(floor_to_multiple(-37i64, Pow2::from_exponent(5)), -64);
        assert_eq!(floor_to_multiple(-32i64, Pow2::from_exponent(5)), -32);
    }

    #[test]
    fn floor_to_multiple_by_one_is_identity() {
        assert_eq!(floor_to_multiple(37i64, Pow2::from_exponent(0)), 37);
        assert_eq!(floor_to_multiple(-37i64, Pow2::from_exponent(0)), -37);
    }

    #[test]
    fn ceil_to_multiple_u64_already_aligned() {
        assert_eq!(ceil_to_multiple(64u64, Pow2::from_exponent(6)), 64);
    }

    #[test]
    fn ceil_to_multiple_u64_unaligned() {
        assert_eq!(ceil_to_multiple(65u64, Pow2::from_exponent(6)), 128);
        assert_eq!(ceil_to_multiple(1u64, Pow2::from_exponent(6)), 64);
    }

    #[test]
    fn ceil_to_multiple_u64_zero() {
        assert_eq!(ceil_to_multiple(0u64, Pow2::from_exponent(5)), 0);
    }

    #[test]
    fn ceil_to_multiple_i64_positive() {
        assert_eq!(ceil_to_multiple(33i64, Pow2::from_exponent(5)), 64);
        assert_eq!(ceil_to_multiple(32i64, Pow2::from_exponent(5)), 32);
    }

    #[test]
    fn mod_floor_u64_exact() {
        assert_eq!(mod_floor(32u64, Pow2::from_exponent(5)), 0);
    }

    #[test]
    fn mod_floor_u64_remainder() {
        assert_eq!(mod_floor(37u64, Pow2::from_exponent(5)), 5);
        assert_eq!(mod_floor(63u64, Pow2::from_exponent(5)), 31);
    }

    #[test]
    fn mod_floor_i64_positive() {
        assert_eq!(mod_floor(37i64, Pow2::from_exponent(5)), 5);
    }

    #[test]
    fn mod_floor_i64_negative_is_always_nonnegative() {
        // div_floor(-37, 32) = -64, so mod = -37 - (-64) = 27
        assert_eq!(mod_floor(-37i64, Pow2::from_exponent(5)), 27);
        // div_floor(-32, 32) = -32, so mod = 0
        assert_eq!(mod_floor(-32i64, Pow2::from_exponent(5)), 0);
        // div_floor(-1, 32) = -32, so mod = 31
        assert_eq!(mod_floor(-1i64, Pow2::from_exponent(5)), 31);
    }

    #[test]
    fn is_multiple_of_u64_true() {
        assert!(is_multiple_of(0u64, Pow2::from_exponent(5)));
        assert!(is_multiple_of(32u64, Pow2::from_exponent(5)));
        assert!(is_multiple_of(64u64, Pow2::from_exponent(5)));
    }

    #[test]
    fn is_multiple_of_u64_false() {
        assert!(!is_multiple_of(31u64, Pow2::from_exponent(5)));
        assert!(!is_multiple_of(33u64, Pow2::from_exponent(5)));
    }

    #[test]
    fn is_multiple_of_i64_negative() {
        assert!(is_multiple_of(-32i64, Pow2::from_exponent(5)));
        assert!(!is_multiple_of(-31i64, Pow2::from_exponent(5)));
    }

    #[test]
    fn div_floor_consistent_with_floor_to_multiple() {
        let b = Pow2::from_exponent(5); // 32
        for a in (-200i64..=200).step_by(7) {
            // floor_to_multiple gives the floored dividend times the divisor,
            // so recover the quotient via div_floor itself rather than `/`
            // (plain `/` truncates toward zero, not toward -∞).
            let q = div_floor(a, b);
            assert_eq!(floor_to_multiple(a, b), q * 32, "mismatch at a={a}");
        }
    }

    #[test]
    fn floor_and_ceil_to_multiple_bracket_the_input() {
        let b = Pow2::from_exponent(4); // 16
        for a in 0u64..=200 {
            let lo = floor_to_multiple(a, b);
            let hi = ceil_to_multiple(a, b);
            assert!(lo <= a, "floor {lo} > {a}");
            assert!(hi >= a, "ceil {hi} < {a}");
            assert!(
                hi - lo == 0 || hi - lo == 16,
                "gap between floor and ceil should be 0 or 16, got {} for a={a}",
                hi - lo
            );
        }
    }

    #[test]
    fn mod_floor_plus_floor_to_multiple_equals_input() {
        let b = Pow2::from_exponent(5); // 32
        for a in (-300i64..=300).step_by(11) {
            let reconstructed = floor_to_multiple(a, b) + mod_floor(a, b);
            assert_eq!(reconstructed, a, "reconstruction failed at a={a}");
        }
    }
}
