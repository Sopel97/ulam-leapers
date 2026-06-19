use crate::math::pow2::{div_floor, Pow2};
use eframe::egui::Color32;
use std::ops::AddAssign;
use std::sync::LazyLock;

/// Accumulates and averages sRGBA [Color32] values in 16-bit linear color.
#[repr(align(32))]
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub struct Color32Accumulator {
    r: u64,
    g: u64,
    b: u64,
    a: u64,
}

impl Color32Accumulator {
    pub fn from_srgb(color: Color32) -> Self {
        Self {
            r: SRGB8_TO_LINEAR16[color.r() as usize] as u64,
            g: SRGB8_TO_LINEAR16[color.g() as usize] as u64,
            b: SRGB8_TO_LINEAR16[color.b() as usize] as u64,
            a: color.a() as u64,
        }
    }

    pub fn zero() -> Self {
        Self {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        }
    }

    pub fn average_to_srgb(&self, count: usize) -> Color32 {
        Color32::from_rgba_premultiplied(
            LINEAR16_TO_SRGB8[(self.r / count as u64) as usize],
            LINEAR16_TO_SRGB8[(self.g / count as u64) as usize],
            LINEAR16_TO_SRGB8[(self.b / count as u64) as usize],
            (self.a / count as u64) as u8,
        )
    }

    pub fn average_to_srgb_pow2_count(&self, count: Pow2) -> Color32 {
        Color32::from_rgba_premultiplied(
            LINEAR16_TO_SRGB8[div_floor(self.r, count) as usize],
            LINEAR16_TO_SRGB8[div_floor(self.g, count) as usize],
            LINEAR16_TO_SRGB8[div_floor(self.b, count) as usize],
            div_floor(self.a, count) as u8,
        )
    }
}

impl AddAssign for Color32Accumulator {
    fn add_assign(&mut self, rhs: Color32Accumulator) {
        self.r += rhs.r;
        self.g += rhs.g;
        self.b += rhs.b;
        self.a += rhs.a;
    }
}

pub static SRGB8_TO_LINEAR16: LazyLock<[u16; 256]> = LazyLock::new(|| {
    std::array::from_fn(|i| {
        let s = i as f32 / 255.0;
        let l = if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        };
        let linear16 = (l * 65535.0).round();
        assert!((0.0..=65535.0).contains(&linear16));
        linear16 as u16
    })
});

pub static LINEAR16_TO_SRGB8: LazyLock<[u8; 65536]> = LazyLock::new(|| {
    std::array::from_fn(|i| {
        let l = i as f32 / 65535.0;
        let s = if l <= 0.0031308 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        let srgb8 = (s * 255.0).round();
        assert!((0.0..=255.0).contains(&srgb8));
        srgb8 as u8
    })
});

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color32 = Color32::from_rgba_premultiplied(255, 0, 0, 255);
    const GREEN: Color32 = Color32::from_rgba_premultiplied(0, 255, 0, 255);
    const BLUE: Color32 = Color32::from_rgba_premultiplied(0, 0, 255, 255);
    const BLACK: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 255);
    const TRANSPARENT: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 0);

    const SRGB_MIDPOINT: u8 = 188;
    const ALPHA_MIDPOINT: u8 = 127;

    #[test]
    fn lookup_table_has_correct_boundary_values() {
        assert_eq!(SRGB8_TO_LINEAR16[0], 0);
        assert_eq!(SRGB8_TO_LINEAR16[u8::MAX as usize], u16::MAX);
        assert_eq!(LINEAR16_TO_SRGB8[0], 0);
        assert_eq!(LINEAR16_TO_SRGB8[u16::MAX as usize], u8::MAX);
    }

    #[test]
    fn test_r_gamma_corrected() {
        let mut acc = Color32Accumulator::zero();
        acc += Color32Accumulator::from_srgb(RED);
        acc += Color32Accumulator::from_srgb(BLACK);
        acc += Color32Accumulator::from_srgb(RED);
        acc += Color32Accumulator::from_srgb(BLACK);
        let col = acc.average_to_srgb(4);
        assert_eq!(col.r(), SRGB_MIDPOINT);
        assert_eq!(col.g(), 0);
        assert_eq!(col.b(), 0);
        assert_eq!(col.a(), 255);
    }

    #[test]
    fn test_g_gamma_corrected() {
        let mut acc = Color32Accumulator::zero();
        acc += Color32Accumulator::from_srgb(GREEN);
        acc += Color32Accumulator::from_srgb(BLACK);
        acc += Color32Accumulator::from_srgb(GREEN);
        acc += Color32Accumulator::from_srgb(BLACK);
        let col = acc.average_to_srgb(4);
        assert_eq!(col.r(), 0);
        assert_eq!(col.g(), SRGB_MIDPOINT);
        assert_eq!(col.b(), 0);
        assert_eq!(col.a(), 255);
    }

    #[test]
    fn test_b_gamma_corrected() {
        let mut acc = Color32Accumulator::zero();
        acc += Color32Accumulator::from_srgb(BLUE);
        acc += Color32Accumulator::from_srgb(BLACK);
        acc += Color32Accumulator::from_srgb(BLUE);
        acc += Color32Accumulator::from_srgb(BLACK);
        let col = acc.average_to_srgb(4);
        assert_eq!(col.r(), 0);
        assert_eq!(col.g(), 0);
        assert_eq!(col.b(), SRGB_MIDPOINT);
        assert_eq!(col.a(), 255);
    }

    #[test]
    fn test_a_not_gamma_corrected() {
        let mut acc = Color32Accumulator::zero();
        acc += Color32Accumulator::from_srgb(TRANSPARENT);
        acc += Color32Accumulator::from_srgb(BLACK);
        acc += Color32Accumulator::from_srgb(TRANSPARENT);
        acc += Color32Accumulator::from_srgb(BLACK);
        let col = acc.average_to_srgb(4);
        assert_eq!(col.r(), 0);
        assert_eq!(col.g(), 0);
        assert_eq!(col.b(), 0);
        assert_eq!(col.a(), ALPHA_MIDPOINT);
    }
}