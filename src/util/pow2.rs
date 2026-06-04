use std::ops::{Add, BitAnd, Shl, Shr, Sub};

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Copy)]
pub struct Pow2 {
    exponent: u8,
}

impl Pow2 {
    pub const fn new(val: usize) -> Pow2 {
        if !val.is_power_of_two() {
            panic!("The provided value is not a power of two");
        }

        Pow2 {
            exponent: val.ilog2() as u8,
        }
    }

    pub const fn from_exponent(exponent: usize) -> Pow2 {
        if exponent > 63 {
            panic!("The provided exponent is too large");
        }

        Pow2 {
            exponent: exponent as u8,
        }
    }

    pub const fn floor_mod_mask(&self) -> usize {
        (1usize << self.exponent) - 1
    }
    
    pub fn exponent(&self) -> u8 {
        self.exponent
    }
    
    pub fn as_usize(&self) -> usize {
        1usize << self.exponent
    }

    pub fn next(self) -> Pow2 {
        Pow2 { exponent: self.exponent + 1 }
    }
}

impl From<Pow2> for u8 {
    fn from(p: Pow2) -> Self { 1u8 << p.exponent }
}

impl From<Pow2> for i8 {
    fn from(p: Pow2) -> Self { 1i8 << p.exponent }
}

impl From<Pow2> for u16 {
    fn from(p: Pow2) -> Self { 1u16 << p.exponent }
}

impl From<Pow2> for i16 {
    fn from(p: Pow2) -> Self { 1i16 << p.exponent }
}

impl From<Pow2> for u32 {
    fn from(p: Pow2) -> Self { 1u32 << p.exponent }
}

impl From<Pow2> for i32 {
    fn from(p: Pow2) -> Self { 1i32 << p.exponent }
}

impl From<Pow2> for u64 {
    fn from(p: Pow2) -> Self { 1u64 << p.exponent }
}

impl From<Pow2> for i64 {
    fn from(p: Pow2) -> Self { 1i64 << p.exponent }
}

impl From<Pow2> for usize {
    fn from(p: Pow2) -> Self { 1usize << p.exponent }
}

impl From<Pow2> for isize {
    fn from(p: Pow2) -> Self { 1isize << p.exponent }
}

pub fn floor_div<T>(a: T, b: Pow2) -> T
where
    T: Shr<Output = T> + From<u8>,
{
    a >> T::from(b.exponent)
}

pub fn floor_mod<T>(a: T, b: Pow2) -> T
where
    T: BitAnd<Output = T> + Shl<Output = T> + Sub<Output = T> + From<u8>,
{
    a & ((T::from(1) << T::from(b.exponent)) - T::from(1))
}

pub fn floor_to_multiple<T>(a: T, b: Pow2) -> T
where
    T: Shr<Output = T> + Shl<Output = T> + From<u8> + Copy,
{
    let exp = T::from(b.exponent);
    a >> exp << exp
}

pub fn ceil_to_multiple<T>(a: T, b: Pow2) -> T
where
    T: Shr<Output = T> + Shl<Output = T> + Add<Output = T> + Sub<Output = T> + From<u8> + From<Pow2> + Copy,
{
    let exp = T::from(b.exponent);
    let v: T = b.into();
    (a + (v - T::from(1))) >> exp << exp
}
