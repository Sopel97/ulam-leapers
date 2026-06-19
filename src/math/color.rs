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
        Color32::from_rgba_unmultiplied(
            LINEAR16_TO_SRGB8[(self.r / count as u64) as usize],
            LINEAR16_TO_SRGB8[(self.g / count as u64) as usize],
            LINEAR16_TO_SRGB8[(self.b / count as u64) as usize],
            (self.a / count as u64) as u8,
        )
    }

    pub fn average_to_srgb_pow2_count(&self, count: Pow2) -> Color32 {
        Color32::from_rgba_unmultiplied(
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
