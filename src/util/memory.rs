use std::fmt::{Display, Formatter};
use std::iter::Sum;
use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Sub, SubAssign};
use std::slice;

/// # SAFETY
///
/// See [`slice::from_raw_parts()`] for safety and usage.
pub unsafe fn view_as_bytes<T>(slice: &[T]) -> &[u8] {
    unsafe { slice::from_raw_parts(slice.as_ptr() as *const u8, size_of_val(slice)) }
}

/// # SAFETY
///
/// See [`slice::from_raw_parts_mut()`] for safety and usage.
pub unsafe fn view_as_bytes_mut<T>(slice: &mut [T]) -> &mut [u8] {
    unsafe { slice::from_raw_parts_mut(slice.as_mut_ptr() as *mut u8, size_of_val(slice)) }
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
pub struct MemSize {
    bytes: usize,
}

macro_rules! make_constructor {
    ($name: ident, $unit: expr) => {
        pub const fn $name(count: usize) -> Self {
            Self {
                bytes: count * ($unit),
            }
        }
    };
}

impl Default for MemSize {
    fn default() -> Self {
        MemSize::ZERO
    }
}

impl MemSize {
    pub const ZERO: Self = Self { bytes: 0 };

    make_constructor!(b, 1);
    make_constructor!(kb, 1_000);
    make_constructor!(mb, 1_000_000);
    make_constructor!(gb, 1_000_000_000);
    make_constructor!(tb, 1_000_000_000_000);
    make_constructor!(pb, 1_000_000_000_000_000);
    make_constructor!(eb, 1_000_000_000_000_000_000);

    make_constructor!(kib, 1 << 10);
    make_constructor!(mib, 1 << 20);
    make_constructor!(gib, 1 << 30);
    make_constructor!(tib, 1 << 40);
    make_constructor!(pib, 1 << 50);
    make_constructor!(eib, 1 << 60);

    pub const fn size_of<T>() -> Self {
        Self {
            bytes: size_of::<T>(),
        }
    }

    pub const fn sizes_of<T>(count: usize) -> Self {
        Self {
            bytes: count * size_of::<T>(),
        }
    }
}

impl MemSize {
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    pub const fn display(&self) -> MemSizeDisplay {
        MemSizeDisplay::new(self.bytes)
    }
}

impl Add<MemSize> for MemSize {
    type Output = MemSize;
    fn add(self, rhs: MemSize) -> MemSize {
        MemSize {
            bytes: self.bytes + rhs.bytes,
        }
    }
}

impl AddAssign<MemSize> for MemSize {
    fn add_assign(&mut self, rhs: MemSize) {
        self.bytes += rhs.bytes;
    }
}

impl Sub<MemSize> for MemSize {
    type Output = MemSize;
    fn sub(self, rhs: MemSize) -> MemSize {
        MemSize {
            bytes: self.bytes - rhs.bytes,
        }
    }
}

impl SubAssign<MemSize> for MemSize {
    fn sub_assign(&mut self, rhs: MemSize) {
        self.bytes -= rhs.bytes;
    }
}

impl Mul<usize> for MemSize {
    type Output = MemSize;
    fn mul(self, rhs: usize) -> Self::Output {
        MemSize {
            bytes: self.bytes * rhs,
        }
    }
}

impl MulAssign<usize> for MemSize {
    fn mul_assign(&mut self, rhs: usize) {
        self.bytes *= rhs;
    }
}

impl Sum<MemSize> for MemSize {
    fn sum<I: Iterator<Item = MemSize>>(iter: I) -> Self {
        let mut total_bytes = 0;
        for bytes in iter {
            total_bytes += bytes.bytes;
        }
        MemSize { bytes: total_bytes }
    }
}

impl Div<MemSize> for MemSize {
    type Output = f64;
    fn div(self, rhs: MemSize) -> Self::Output {
        self.bytes as f64 / rhs.bytes as f64
    }
}

const DEFAULT_PRECISION: usize = 3;
// Normally we would use U+202F NARROW NO-BREAK SPACE (NNBSP)
// but the support for it is terrible.
// In particular egui does not seem to support it on Windows by default.
// We use the normal NBSP instead.
const DEFAULT_SEPARATOR: &str = "\u{00A0}";
const SUFFIX_SI: &str = "B";
const SUFFIX_IEC: &str = "iB";

pub enum MemSizeDisplaySuffixFormat {
    Si,
    Iec,
}

pub struct MemSizeDisplay {
    bytes: usize,
    precision: usize,
    separator: &'static str,
    suffix_format: MemSizeDisplaySuffixFormat,
}

impl MemSizeDisplay {
    pub const fn new(bytes: usize) -> Self {
        Self {
            bytes,
            separator: DEFAULT_SEPARATOR,
            precision: DEFAULT_PRECISION,
            suffix_format: MemSizeDisplaySuffixFormat::Si,
        }
    }

    pub const fn si(self) -> Self {
        Self {
            suffix_format: MemSizeDisplaySuffixFormat::Si,
            ..self
        }
    }

    pub const fn iec(self) -> Self {
        Self {
            suffix_format: MemSizeDisplaySuffixFormat::Iec,
            ..self
        }
    }

    pub const fn precision(self, new_precision: usize) -> Self {
        Self {
            precision: new_precision,
            ..self
        }
    }

    pub const fn separator(self, new_separator: &'static str) -> Self {
        Self {
            separator: new_separator,
            ..self
        }
    }
}

impl MemSizeDisplay {
    pub const fn best_prefix(&self) -> (&'static str, usize) {
        match self.suffix_format {
            MemSizeDisplaySuffixFormat::Si => match self.bytes {
                b if b < 1_000 => ("", 1),
                b if b < 1_000_000 => ("K", 1_000),
                b if b < 1_000_000_000 => ("M", 1_000_000),
                b if b < 1_000_000_000_000 => ("G", 1_000_000_000),
                b if b < 1_000_000_000_000_000 => ("T", 1_000_000_000_000),
                b if b < 1_000_000_000_000_000_000 => ("P", 1_000_000_000_000_000),
                _ => ("E", 1_000_000_000_000_000_000),
            },
            MemSizeDisplaySuffixFormat::Iec => match self.bytes {
                b if b < (1 << 10) => ("", 1),
                b if b < (1 << 20) => ("K", 1 << 10),
                b if b < (1 << 30) => ("M", 1 << 20),
                b if b < (1 << 40) => ("G", 1 << 30),
                b if b < (1 << 50) => ("T", 1 << 40),
                b if b < (1 << 60) => ("P", 1 << 50),
                _ => ("E", 1 << 60),
            },
        }
    }
}

/// Formats a number with `sig` significant digits.
fn format_sig(x: f64, sig: usize) -> String {
    if x == 0.0 {
        return "0".to_string();
    }

    let digits = sig as i32 - 1 - x.abs().log10().floor() as i32;
    let digits = digits.max(0) as usize;

    let s = format!("{:.*}", digits, x);

    // remove trailing zeros after decimal point
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

impl Display for MemSizeDisplay {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let (prefix, unit) = self.best_prefix();

        let value = format_sig(self.bytes as f64 / unit as f64, self.precision);

        let suffix = match self.suffix_format {
            MemSizeDisplaySuffixFormat::Si => SUFFIX_SI,
            MemSizeDisplaySuffixFormat::Iec => SUFFIX_IEC,
        };

        let separator = self.separator;

        write!(f, "{value}{separator}{prefix}{suffix}",)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_si_constructors() {
        assert_eq!(MemSize::b(1).bytes(), 1);
        assert_eq!(MemSize::kb(1).bytes(), 1_000);
        assert_eq!(MemSize::mb(1).bytes(), 1_000_000);
        assert_eq!(MemSize::gb(1).bytes(), 1_000_000_000);
        assert_eq!(MemSize::tb(1).bytes(), 1_000_000_000_000);
        assert_eq!(MemSize::pb(1).bytes(), 1_000_000_000_000_000);
        assert_eq!(MemSize::eb(1).bytes(), 1_000_000_000_000_000_000);
    }

    #[test]
    fn test_iec_constructors() {
        assert_eq!(MemSize::kib(1).bytes(), 1 << 10);
        assert_eq!(MemSize::mib(1).bytes(), 1 << 20);
        assert_eq!(MemSize::gib(1).bytes(), 1 << 30);
        assert_eq!(MemSize::tib(1).bytes(), 1 << 40);
        assert_eq!(MemSize::pib(1).bytes(), 1 << 50);
        assert_eq!(MemSize::eib(1).bytes(), 1 << 60);
    }

    #[test]
    fn test_size_of() {
        assert_eq!(MemSize::size_of::<u8>().bytes(), 1);
        assert_eq!(MemSize::size_of::<u32>().bytes(), 4);
        assert_eq!(MemSize::size_of::<u64>().bytes(), 8);
        assert_eq!(MemSize::size_of::<u128>().bytes(), 16);
    }

    #[test]
    fn test_sizes_of() {
        assert_eq!(MemSize::sizes_of::<u8>(0).bytes(), 0);
        assert_eq!(MemSize::sizes_of::<u8>(100).bytes(), 100);
        assert_eq!(MemSize::sizes_of::<u32>(4).bytes(), 16);
        assert_eq!(MemSize::sizes_of::<u64>(3).bytes(), 24);
    }

    #[test]
    fn test_zero_count_constructors() {
        assert_eq!(MemSize::b(0).bytes(), 0);
        assert_eq!(MemSize::kb(0).bytes(), 0);
        assert_eq!(MemSize::mib(0).bytes(), 0);
    }

    #[test]
    fn test_add() {
        let a = MemSize::kb(1);
        let b = MemSize::b(500);
        assert_eq!((a + b).bytes(), 1_500);
    }

    #[test]
    fn test_add_assign() {
        let mut a = MemSize::mb(1);
        a += MemSize::kb(500);
        assert_eq!(a.bytes(), 1_500_000);
    }

    #[test]
    fn test_sub() {
        let a = MemSize::kb(2);
        let b = MemSize::b(500);
        assert_eq!((a - b).bytes(), 1_500);
    }

    #[test]
    fn test_sub_assign() {
        let mut a = MemSize::mb(1);
        a -= MemSize::kb(100);
        assert_eq!(a.bytes(), 900_000);
    }

    #[test]
    #[allow(clippy::erasing_op)]
    fn test_mul() {
        assert_eq!((MemSize::kb(1) * 4).bytes(), 4_000);
        assert_eq!((MemSize::mib(1) * 0).bytes(), 0);
    }

    #[test]
    fn test_mul_assign() {
        let mut a = MemSize::kb(3);
        a *= 5;
        assert_eq!(a.bytes(), 15_000);
    }

    #[test]
    fn test_ordering() {
        assert!(MemSize::kb(1) < MemSize::mb(1));
        assert!(MemSize::gb(1) > MemSize::mb(999));
        assert_eq!(MemSize::kib(1), MemSize::b(1024));
    }

    #[test]
    fn test_sum_empty() {
        let total: MemSize = std::iter::empty::<MemSize>().sum();
        assert_eq!(total.bytes(), 0);
    }

    #[test]
    fn test_sum_several() {
        let sizes = vec![
            MemSize::kb(1),
            MemSize::kb(2),
            MemSize::kb(3),
            MemSize::b(4),
        ];
        let total: MemSize = sizes.into_iter().sum();
        assert_eq!(total.bytes(), 6_004);
    }

    #[test]
    fn test_best_prefix_si_bytes() {
        let d = MemSizeDisplay::new(999);
        let (prefix, unit) = d.best_prefix();
        assert_eq!(prefix, "");
        assert_eq!(unit, 1);
    }

    #[test]
    fn test_best_prefix_si_kilo() {
        let d = MemSizeDisplay::new(1_000);
        let (prefix, unit) = d.best_prefix();
        assert_eq!(prefix, "K");
        assert_eq!(unit, 1_000);
    }

    #[test]
    fn test_best_prefix_si_mega() {
        let d = MemSizeDisplay::new(1_000_000);
        let (prefix, unit) = d.best_prefix();
        assert_eq!(prefix, "M");
        assert_eq!(unit, 1_000_000);
    }

    #[test]
    fn test_best_prefix_si_giga() {
        let d = MemSizeDisplay::new(1_000_000_000);
        let (prefix, unit) = d.best_prefix();
        assert_eq!(prefix, "G");
        assert_eq!(unit, 1_000_000_000);
    }

    #[test]
    fn test_best_prefix_si_exa() {
        let d = MemSizeDisplay::new(usize::MAX); // well into exa range
        let (prefix, unit) = d.best_prefix();
        assert_eq!(prefix, "E");
        assert_eq!(unit, 1_000_000_000_000_000_000);
    }

    #[test]
    fn test_best_prefix_iec_bytes() {
        let d = MemSizeDisplay::new(1023).iec();
        let (prefix, unit) = d.best_prefix();
        assert_eq!(prefix, "");
        assert_eq!(unit, 1);
    }

    #[test]
    fn test_best_prefix_iec_kibi() {
        let d = MemSizeDisplay::new(1024).iec();
        let (prefix, unit) = d.best_prefix();
        assert_eq!(prefix, "K");
        assert_eq!(unit, 1 << 10);
    }

    #[test]
    fn test_best_prefix_iec_mebi() {
        let d = MemSizeDisplay::new(1 << 20).iec();
        let (prefix, unit) = d.best_prefix();
        assert_eq!(prefix, "M");
        assert_eq!(unit, 1 << 20);
    }

    // The separator is normal NBSP because the NARROW NO-BREAK SPACE (U+202F) has low font support.
    const SEP: &str = "\u{00A0}";

    #[test]
    fn test_display_si_bytes() {
        let s = MemSize::b(512).display().si().to_string();
        // 512 B - no prefix, precision 3 means one decimal digit for values < 1000
        assert!(s.contains("512"), "expected 512 in '{s}'");
        assert!(
            s.ends_with(&format!("{SEP}B")),
            "expected SI suffix in '{s}'"
        );
    }

    #[test]
    fn test_display_si_kilobytes() {
        let s = MemSize::kb(1).display().si().to_string();
        assert!(s.contains('1'), "expected value in '{s}'");
        assert!(
            s.ends_with(&format!("{SEP}KB")),
            "expected KB suffix in '{s}'"
        );
    }

    #[test]
    fn test_display_si_megabytes() {
        let s = MemSize::mb(1).display().si().to_string();
        assert!(
            s.ends_with(&format!("{SEP}MB")),
            "expected MB suffix in '{s}'"
        );
    }

    #[test]
    fn test_display_iec_kibibytes() {
        let s = MemSize::kib(1).display().iec().to_string();
        assert!(
            s.ends_with(&format!("{SEP}KiB")),
            "expected KiB suffix in '{s}'"
        );
    }

    #[test]
    fn test_display_iec_mebibytes() {
        let s = MemSize::mib(4).display().iec().to_string();
        assert!(
            s.ends_with(&format!("{SEP}MiB")),
            "expected MiB suffix in '{s}'"
        );
        assert!(s.contains('4'), "expected value 4 in '{s}'");
    }

    #[test]
    fn test_display_precision_three_significant_digits() {
        // 1.50 KB: value = 1.5, precision 3 → 1 decimal digit
        let s = MemSize::b(1_500).display().si().to_string();
        assert!(
            s.starts_with("1.50") || s.starts_with("1.5"),
            "expected ~1.5 KB, got '{s}'"
        );
    }

    #[test]
    fn test_display_default_is_si() {
        // Default format should use SI (B suffix, not iB)
        let s = MemSize::mb(1).display().to_string();
        assert!(!s.contains("iB"), "default should be SI, got '{s}'");
        assert!(s.contains('B'), "default should include B suffix in '{s}'");
    }

    #[test]
    fn test_specific_display_cases() {
        assert_eq!(MemSize::b(0).display().si().to_string(), format!("0{SEP}B"));
        assert_eq!(
            MemSize::gb(3).display().si().to_string(),
            format!("3{SEP}GB")
        );
        assert_eq!(
            MemSize::gib(3).display().iec().to_string(),
            format!("3{SEP}GiB")
        );
        assert_eq!(
            MemSize::kb(1559).display().si().precision(2).to_string(),
            format!("1.6{SEP}MB")
        );
        assert_eq!(
            MemSize::kb(1559).display().si().precision(3).to_string(),
            format!("1.56{SEP}MB")
        );
        assert_eq!(
            MemSize::kb(1559).display().si().precision(4).to_string(),
            format!("1.559{SEP}MB")
        );
        assert_eq!(
            MemSize::kb(1559).display().si().precision(10).to_string(),
            format!("1.559{SEP}MB")
        );
        assert_eq!(
            MemSize::gb(59).display().si().precision(0).to_string(),
            format!("59{SEP}GB")
        );
        assert_eq!(
            MemSize::gb(59).display().si().precision(2).to_string(),
            format!("59{SEP}GB")
        );
        assert_eq!(
            MemSize::gb(59).display().si().precision(4).to_string(),
            format!("59{SEP}GB")
        );
    }

    #[test]
    fn test_display_via_memsize_helper() {
        // MemSize::display() is a convenience wrapper; output must match direct construction
        let via_helper = MemSize::gb(2).display().to_string();
        let direct = MemSizeDisplay::new(MemSize::gb(2).bytes()).to_string();
        assert_eq!(via_helper, direct);
    }
}
