use crate::math::pow2::Pow2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zoom<T>
where
    T: Copy,
{
    Magnification(T),
    Minification(T),
}

impl Zoom<Pow2> {
    pub fn from_exponent(zoom_pow2: i32) -> Zoom<Pow2> {
        match zoom_pow2 {
            e @ 0.. => Zoom::Magnification(Pow2::from_exponent(e as u8)),
            e @ ..0 => Zoom::Minification(Pow2::from_exponent((-e) as u8)),
        }
    }
}
