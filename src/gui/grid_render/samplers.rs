use eframe::egui::Color32;
use ulam_leapers::game::sampler::SampleCollector;
use ulam_leapers::game::simulation::PlayerId;
use ulam_leapers::math::color::Color32Accumulator;
use ulam_leapers::math::pow2::Pow2;

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

#[derive(Debug)]
pub struct AvgMapColor32Collector {
    colors_linear_u64: Vec<Color32Accumulator>,
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
            .map(|c| Color32Accumulator::from_srgb(*c))
            .collect::<Vec<_>>();

        // Prefill to allow avoiding bound checks.
        colors_linear_u64.resize(u8::MAX as usize + 1, Color32Accumulator::zero());

        Self { colors_linear_u64 }
    }
}

impl SampleCollector for AvgMapColor32Collector {
    type InputType = PlayerId;
    type AccumulatorType = Color32Accumulator;
    type OutputType = Color32;

    #[inline(always)]
    fn zero(&self) -> Self::AccumulatorType {
        Color32Accumulator::zero()
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
