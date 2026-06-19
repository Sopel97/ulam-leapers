use std::sync::LazyLock;

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