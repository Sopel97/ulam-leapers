use crate::math::pow2::Pow2;
use std::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zoom<T>
where
    T: Copy,
{
    Magnification(T),
    Minification(T),
}

impl<T> Display for Zoom<T>
where
    T: Display + Copy,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Zoom::Magnification(factor) => {
                write!(f, "{factor}x")
            }
            Zoom::Minification(factor) => {
                write!(f, "1/{factor}x")
            }
        }
    }
}

impl Zoom<Pow2> {
    pub fn from_exponent(zoom_pow2: i32) -> Zoom<Pow2> {
        match zoom_pow2 {
            e @ 0.. => Zoom::Magnification(Pow2::from_exponent(e as u8)),
            e @ ..0 => Zoom::Minification(Pow2::from_exponent((-e) as u8)),
        }
    }
}
