use std::ops::AddAssign;
use eframe::egui::Color32;
use ulam_leapers::game::sampler::SampleCollector;
use ulam_leapers::game::simulation::PlayerId;
use ulam_leapers::math::color::{LINEAR16_TO_SRGB8, SRGB8_TO_LINEAR16};
use ulam_leapers::math::pow2::{div_floor, Pow2};

#[derive(Debug)]
pub struct MapLastCollector<'a, T> {
    colors: &'a [T],
}

impl<'a, T> MapLastCollector<'a, T> {
    pub fn new(colors: &'a [T]) -> Self {
        MapLastCollector { colors }
    }
}

impl<'a, T> SampleCollector for MapLastCollector<'a, T>
where
    T: Default + Copy + Send + Sync + 'static,
{
    type InputType = PlayerId;
    type AccumulatorType = T;
    type OutputType = T;

    fn zero(&self) -> Self::AccumulatorType {
        Default::default()
    }

    fn push(&self, acc: &mut Self::AccumulatorType, input: Self::InputType) {
        *acc = self.colors[input.index()]
    }

    fn finalize(&self, acc: Self::AccumulatorType, _size: (usize, usize)) -> Self::OutputType {
        acc
    }
}

#[repr(align(16))]
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub struct AccColSrgb8 {
    r: u32,
    g: u32,
    b: u32,
    a: u32,
}

#[repr(align(32))]
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub struct AccColLinear16 {
    r: u64,
    g: u64,
    b: u64,
    a: u64,
}

impl AccColLinear16 {
    pub fn from_srgb(color: Color32) -> Self{
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

impl AddAssign for AccColLinear16 {
    fn add_assign(&mut self, rhs: AccColLinear16) {
        self.r += rhs.r;
        self.g += rhs.g;
        self.b += rhs.b;
        self.a += rhs.a;
    }
}

#[derive(Debug)]
pub struct AvgMapColor32Collector {
    colors_linear_u64: Vec<AccColLinear16>,
}

/// This collector maps player IDs into colors using a lookup table.
/// Due to performance considerations this lookup is unchecked.
/// Safety is ensured by overallocating colors to account for every
/// possible player ID. If a player ID which the user-provided `colors`
/// do not account for the result implementation-defined.
/// This is only viable because player ID is one byte.
impl AvgMapColor32Collector {
    pub fn new(colors: &[Color32]) -> Self {
        // We require that the size of player IDs is reasonable,
        // because we prefill all of them to avoid bound checks.
        assert_eq!(size_of::<PlayerId>(), 1);

        let mut colors_linear_u64 = colors
            .iter()
            .map(|c| AccColLinear16::from_srgb(*c))
            .collect::<Vec<_>>();

        // Prefill to allow avoiding bound checks.
        colors_linear_u64.resize(
            u8::MAX as usize + 1,
            AccColLinear16::zero(),
        );

        Self { colors_linear_u64 }
    }
}

impl SampleCollector for AvgMapColor32Collector {
    type InputType = PlayerId;
    type AccumulatorType = AccColLinear16;
    type OutputType = Color32;

    #[inline(always)]
    fn zero(&self) -> Self::AccumulatorType {
        AccColLinear16::zero()
    }

    #[inline(always)]
    fn push(&self, acc: &mut Self::AccumulatorType, input: Self::InputType) {
        // SAFETY: We guarantee correctness by prefilling the `self.colors_u32`
        //         array up to the maximum possible player ID.
        let color = unsafe { self.colors_linear_u64.get_unchecked(input.index()) };
        *acc += *color;
    }

    #[inline(always)]
    fn finalize(
        &self,
        acc: Self::AccumulatorType,
        (width, height): (usize, usize),
    ) -> Self::OutputType {
        let count = Pow2::try_from((width * height) as u64).unwrap();
        acc.average_to_srgb_pow2_count(count)
    }
}
