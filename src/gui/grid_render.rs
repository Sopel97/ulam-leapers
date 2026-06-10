use eframe::egui;
use eframe::egui::{
    Color32, ColorImage, TextureFilter, TextureHandle, TextureOptions, TextureWrapMode,
};
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use ulam_leapers::collections::array2d::{Array2D, MutSlice2D};
use ulam_leapers::game::chunk::ChunkOrigin;
use ulam_leapers::game::grid::{FrozenGrid, FrozenGridSampler, SampleCollector};
use ulam_leapers::game::simulation::{FinalizedSimulation, PlayerId};
use ulam_leapers::math::pow2::{ceil_to_multiple, floor_div, floor_to_multiple, Pow2};
use ulam_leapers::math::rect::GridRect;
use ulam_leapers::util::align::CACHE_LINE_SIZE;
use ulam_leapers::util::cache::LockStepCache;
use ulam_leapers::util::cancel::{Canceled, CancellationToken};
use ulam_leapers::util::memory::MemSize;
use ulam_leapers::util::sync::DeferredValue;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    Magnification(Pow2),
    Minification(Pow2),
}
use Zoom::*;

pub fn default_player_colors() -> Vec<Color32> {
    vec![
        Color32::WHITE,
        Color32::BLACK,
        Color32::RED,
        Color32::BLUE,
        Color32::YELLOW,
        Color32::GREEN,
        Color32::CYAN,
        Color32::MAGENTA,
        Color32::BROWN,
    ]
}

type CacheType = LockStepCache<(ChunkOrigin, Pow2), Array2D<Color32>>;
type MipmapStorageType = BTreeMap<Pow2, Array2D<Color32>>;

pub struct Mipmaps {
    by_minification_factor: MipmapStorageType,
    grid_bounds: GridRect,
}

pub struct GridRenderer {
    grid: Arc<FrozenGrid<PlayerId>>,
    highest_player_id: PlayerId,
    colors: Vec<Color32>,
    mipmaps: DeferredValue<Mipmaps>,
    cache: Option<RefCell<CacheType>>,
}

#[repr(align(16))]
struct AccCol {
    r: u32,
    g: u32,
    b: u32,
    a: u32,
}

struct AvgColorCollector {
    colors_u32: Vec<AccCol>,
}

impl AvgColorCollector {
    pub fn new(colors: &[Color32]) -> Self {
        let colors_u32 = colors
            .iter()
            .map(|c| AccCol {
                r: c.r() as u32,
                g: c.g() as u32,
                b: c.b() as u32,
                a: c.a() as u32,
            })
            .collect::<Vec<_>>();

        Self { colors_u32 }
    }
}

impl SampleCollector for AvgColorCollector {
    type InputType = PlayerId;
    type AccumulatorType = AccCol;
    type OutputType = Color32;

    #[inline(always)]
    fn zero(&self) -> Self::AccumulatorType {
        AccCol {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        }
    }

    #[inline(always)]
    fn push(&self, acc: &mut Self::AccumulatorType, input: Self::InputType) {
        // SAFETY: We can't guarantee the safety here,
        //         the caller must make sure there is enough colors.
        //         However, this is a hot loop, we need it for speed.
        let color = unsafe { self.colors_u32.get_unchecked(input.index()) };
        acc.r += color.r;
        acc.g += color.g;
        acc.b += color.b;
        acc.a += color.a;
    }

    #[inline(always)]
    fn finalize(
        &self,
        acc: Self::AccumulatorType,
        (width, height): (usize, usize),
    ) -> Self::OutputType {
        let count = Pow2::new(width * height);
        Color32::from_rgb(
            floor_div(acc.r, count) as u8,
            floor_div(acc.g, count) as u8,
            floor_div(acc.b, count) as u8,
        )
    }
}

struct LastColorCollector<'a> {
    colors: &'a [Color32],
}

impl<'a> SampleCollector for LastColorCollector<'a> {
    type InputType = PlayerId;
    type AccumulatorType = Color32;
    type OutputType = Color32;

    fn zero(&self) -> Self::AccumulatorType {
        Color32::from_rgb(0, 0, 0)
    }

    fn push(&self, acc: &mut Self::AccumulatorType, input: Self::InputType) {
        *acc = self.colors[input.index()]
    }

    fn finalize(&self, acc: Self::AccumulatorType, _size: (usize, usize)) -> Self::OutputType {
        acc
    }
}

pub struct MipmapGenerationProgress {
    slot: Arc<Mutex<(usize, usize)>>,
}

impl MipmapGenerationProgress {
    pub fn new(slot: Arc<Mutex<(usize, usize)>>) -> MipmapGenerationProgress {
        MipmapGenerationProgress { slot }
    }

    pub fn get(&self) -> (usize, usize) {
        *self.slot.lock().unwrap()
    }
}

impl GridRenderer {
    pub fn new(sim: &FinalizedSimulation, colors: &[Color32]) -> Self {
        let highest_player_id = sim.highest_player_id();
        assert!(
            colors.len() > highest_player_id.index(),
            "Not enough colors for this simulation."
        );

        Self {
            grid: sim.grid(),
            highest_player_id,
            colors: colors.to_vec(),
            mipmaps: Default::default(),
            cache: None,
        }
    }

    pub fn set_cache_size(&mut self, max_cache_size: usize) {
        if let Some(cache) = &mut self.cache {
            cache.borrow_mut().set_max_memory_cost(max_cache_size);
        } else {
            self.cache = Some(RefCell::new(LockStepCache::new(max_cache_size)));
        }
    }

    /// Panics if mipmaps have been generated.
    pub fn set_colors(&mut self, colors: &[Color32]) {
        assert!(
            !self.has_mipmaps(),
            "Cannot change colors after mipmaps have been generated."
        );
        assert!(
            colors.len() > self.highest_player_id.index(),
            "Not enough colors for this simulation."
        );

        self.colors = colors.to_vec();
        if let Some(cache) = &mut self.cache {
            cache.borrow_mut().invalidate_all();
        }
    }

    pub fn can_set_colors(&self) -> bool {
        self.can_generate_mipmaps()
    }

    pub fn has_mipmaps(&self) -> bool {
        self.mipmaps.is_ready()
    }

    pub fn mipmap_bounds(&self) -> GridRect {
        self.mipmaps.get().unwrap().grid_bounds
    }

    pub fn highest_mipmap_minification_factor(&self) -> Option<Pow2> {
        self.mipmaps.get()?.by_minification_factor.keys().max().copied()
    }

    fn make_sampler_for_minification(
        &self,
        bounds: &GridRect,
        factor: Pow2,
    ) -> FrozenGridSampler<'_, PlayerId, AvgColorCollector> {
        // u32 is enough for 4096x4096 worst case
        // We do alpha too in case the compiler can vectorize it better than just rgb.
        assert!(
            factor < Pow2::new(4096),
            "Minification too high, could overflow accumulator"
        );

        FrozenGridSampler::new_with_minification(
            &self.grid,
            *bounds,
            factor,
            self.colors[0],
            AvgColorCollector::new(self.colors.as_slice()),
        )
    }

    fn render_to_rgba_samples_for_minification_direct(
        &self,
        bounds: &GridRect,
        factor: Pow2,
    ) -> Array2D<Color32> {
        let sampler = self.make_sampler_for_minification(bounds, factor);

        if let Some(cache) = &self.cache {
            let res = sampler.par_sample_with_cache(&*cache.borrow());
            // We must call update to settle new cached values.
            cache.borrow_mut().update();
            res
        } else {
            sampler.par_sample()
        }
    }

    fn render_to_rgba_samples_for_minification_using_mipmaps(
        &self,
        bounds: &GridRect,
        factor: Pow2,
    ) -> Array2D<Color32> {
        if !bounds.is_aligned_to_pow2(factor) {
            panic!("Region is not aligned to the minification factor.");
        }

        if !self.has_mipmap(factor) {
            panic!("Mipmaps cannot handle this minification factor.");
        }

        let mipmap_bounds = self.mipmap_bounds();

        assert!(mipmap_bounds.is_aligned_to_pow2(factor));

        // We convert the bounds into local coordinates (after minification).
        let mut src_bounds = mipmap_bounds;
        src_bounds.start.x = floor_div(src_bounds.start.x, factor);
        src_bounds.start.y = floor_div(src_bounds.start.y, factor);
        src_bounds.end.x = floor_div(src_bounds.end.x, factor);
        src_bounds.end.y = floor_div(src_bounds.end.y, factor);

        let mut dst_bounds = *bounds;
        dst_bounds.start.x = floor_div(dst_bounds.start.x, factor);
        dst_bounds.start.y = floor_div(dst_bounds.start.y, factor);
        dst_bounds.end.x = floor_div(dst_bounds.end.x, factor);
        dst_bounds.end.y = floor_div(dst_bounds.end.y, factor);

        let intersection = src_bounds.intersection(&dst_bounds).unwrap();

        let mut res = Array2D::<Color32>::new_aligned(
            dst_bounds.width() as usize,
            dst_bounds.height() as usize,
            CACHE_LINE_SIZE,
        );

        // We could maybe make this lock shorter but who cares. This struct is not supposed
        // to be used concurrently anyway.
        let mipmaps = &self.mipmaps.get().unwrap().by_minification_factor;
        let mipmap = mipmaps.get(&factor).unwrap();

        // Every index within the intersection is valid for both src and dst.
        for y in intersection.start.y..intersection.end.y {
            for x in intersection.start.x..intersection.end.x {
                let src_x = (x - src_bounds.start.x) as usize;
                let src_y = (y - src_bounds.start.y) as usize;
                let dst_x = (x - dst_bounds.start.x) as usize;
                let dst_y = (y - dst_bounds.start.y) as usize;

                res[(dst_x, dst_y)] = mipmap[(src_x, src_y)];
            }
        }

        res
    }

    pub fn render_to_rgba_samples(&self, bounds: &GridRect, zoom: Zoom) -> Array2D<Color32> {
        match zoom {
            Magnification(_factor) => {
                let colors = &self.colors;
                let sampler = FrozenGridSampler::new(
                    &self.grid,
                    *bounds,
                    colors[0],
                    LastColorCollector { colors },
                );
                // Do not use a cache for magnification because it's cheap to
                // compute and the results take more memory than the probed chunk cells.
                sampler.par_sample()
            }
            Minification(factor) => {
                if self.has_mipmap(factor) {
                    self.render_to_rgba_samples_for_minification_using_mipmaps(bounds, factor)
                } else {
                    self.render_to_rgba_samples_for_minification_direct(bounds, factor)
                }
            }
        }
    }

    // The caller to this function must guarantee that there are enough colors in
    // params.colors to facilitate every cell. Otherwise, the behavior is undefined.
    pub fn render_to_rgba_image(&self, bounds: &GridRect, zoom: Zoom) -> ColorImage {
        let samples = self.render_to_rgba_samples(bounds, zoom);
        ColorImage::new(
            [samples.width(), samples.height()],
            samples.as_flat_slice().to_vec(),
        )
    }

    pub fn render_texture(
        &mut self,
        ctx: &egui::Context,
        bounds: &GridRect,
        zoom: Zoom,
    ) -> TextureHandle {
        let texture_options = TextureOptions {
            magnification: TextureFilter::Nearest,
            minification: TextureFilter::Linear,
            wrap_mode: TextureWrapMode::ClampToEdge,
            mipmap_mode: None,
        };

        let image = self.render_to_rgba_image(bounds, zoom);
        ctx.load_texture("name", image, texture_options)
    }

    pub fn has_mipmap(&self, factor: Pow2) -> bool {
        match self.mipmaps.get() {
            Some(mipmaps) => mipmaps.by_minification_factor.contains_key(&factor),
            _ => false,
        }
    }

    pub fn estimate_mipmaps_memory_requirement(
        &self,
        lowest_minification: Pow2,
        highest_minification: Pow2,
    ) -> MemSize {
        let mut grid_bounds = self.grid.bounds();
        grid_bounds.start.x = floor_to_multiple(grid_bounds.start.x, highest_minification);
        grid_bounds.start.y = floor_to_multiple(grid_bounds.start.y, highest_minification);
        grid_bounds.end.x = ceil_to_multiple(grid_bounds.end.x, highest_minification);
        grid_bounds.end.y = ceil_to_multiple(grid_bounds.end.y, highest_minification);

        let pixels_at_no_minification =
            grid_bounds.width() as usize * grid_bounds.height() as usize;
        let pixels_at_lowest_minification = floor_div(
            floor_div(pixels_at_no_minification, lowest_minification),
            lowest_minification,
        );

        // 1 + 1/4 + 1/16 + 1/64 + ... converges to 4/3
        MemSize::sizes_of::<Color32>(pixels_at_lowest_minification * 4 / 3)
    }
    
    pub fn can_generate_mipmaps(&self) -> bool {
        self.mipmaps.is_empty_and_idle()
    }
    
    pub fn cancel_mipmap_generation(&mut self) {
        self.mipmaps.try_cancel();
    }

    pub fn generate_mipmaps_async(&mut self, lowest_minification: Pow2, highest_minification: Pow2) -> MipmapGenerationProgress {
        // We allow a single level, but not less than that.
        assert!(lowest_minification <= highest_minification);

        // Strictly speaking we only require the width and height to be aligned to
        // the higher minification factor, but having this constraint on the whole rect
        // is very beneficial for how the Grid behaves.
        let mut grid_bounds = self.grid.bounds();
        grid_bounds.start.x = floor_to_multiple(grid_bounds.start.x, highest_minification);
        grid_bounds.start.y = floor_to_multiple(grid_bounds.start.y, highest_minification);
        grid_bounds.end.x = ceil_to_multiple(grid_bounds.end.x, highest_minification);
        grid_bounds.end.y = ceil_to_multiple(grid_bounds.end.y, highest_minification);

        // Disallow zero-sized bounds
        assert!(grid_bounds.width() > 0);
        assert!(grid_bounds.height() > 0);

        let is_finished = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new((0usize, 1usize)));
        let progress_clone = Arc::clone(&progress);
        let progress_callback = move |done: usize, total: usize| {
            *progress_clone.lock().unwrap() = (done, total);
        };

        let grid_ref = Arc::clone(&self.grid);
        let default_color = self.colors[0];
        let collector = AvgColorCollector::new(self.colors.as_slice());

        let is_finished_clone = Arc::clone(&is_finished);
        let job = move |ct: CancellationToken| {
            let mut mipmaps = MipmapStorageType::new();

            let sampler =
                FrozenGridSampler::new_with_minification(
                    grid_ref.as_ref(),
                    grid_bounds,
                    lowest_minification,
                    default_color,
                    collector,
                );
            let master_mipmap = sampler.par_sample_cancellable(ct.clone(), progress_callback);
            if master_mipmap.is_none() {
                return Err(Canceled);
            }

            mipmaps.insert(
                lowest_minification,
                master_mipmap.unwrap(),
            );
            let mut prev_minification = lowest_minification;
            let mut curr_minification = lowest_minification.next();
            while curr_minification <= highest_minification {
                if ct.is_canceled() {
                    return Err(Canceled);
                }
                
                // We know it exists because we put it either during the init or during the previous iteration.
                let prev_mipmap = mipmaps
                    .get(&prev_minification)
                    .unwrap();

                mipmaps
                    .insert(curr_minification, Self::reduce_mipmap_2x(prev_mipmap));

                prev_minification = curr_minification;
                curr_minification = curr_minification.next();
            }
            
            is_finished_clone.store(true, Ordering::Release);
            
            Ok(Mipmaps {
                by_minification_factor: mipmaps,
                grid_bounds,
            })
        };
        
        match self.mipmaps.try_set_with_async(job) {
            Ok(_) => {}
            Err(err) => panic!("{:?}", err),
        }

        MipmapGenerationProgress::new(progress)
    }

    fn reduce_mipmap_2x(prev_mipmap: &Array2D<Color32>) -> Array2D<Color32> {
        const CHUNK_EXTENT: usize = 1024;
        const MIN_CHUNKS_FOR_PAR: usize = 8; // NOTE: untuned, just guess

        assert!(prev_mipmap.width().is_multiple_of(2));
        assert!(prev_mipmap.height().is_multiple_of(2));

        let lerp4 = |c0: Color32, c1: Color32, c2: Color32, c3: Color32| {
            Color32::from_rgb(
                ((c0.r() as u32 + c1.r() as u32 + c2.r() as u32 + c3.r() as u32) / 4) as u8,
                ((c0.g() as u32 + c1.g() as u32 + c2.g() as u32 + c3.g() as u32) / 4) as u8,
                ((c0.b() as u32 + c1.b() as u32 + c2.b() as u32 + c3.b() as u32) / 4) as u8,
            )
        };

        let kernel = |(base_ox, base_oy, mut chunk): (usize, usize, MutSlice2D<Color32>)| {
            for dy in 0..chunk.height() {
                for dx in 0..chunk.width() {
                    // prev_mipmap is 2x larger
                    let ix = (base_ox + dx) * 2;
                    let iy = (base_oy + dy) * 2;
                    // We need to interpolate a 2x2 area of pixels into 1
                    chunk[(dx, dy)] = lerp4(
                        prev_mipmap[(ix, iy)],
                        prev_mipmap[(ix + 1, iy)],
                        prev_mipmap[(ix, iy + 1)],
                        prev_mipmap[(ix + 1, iy + 1)],
                    );
                }
            }
        };

        let mut curr_mipmap = Array2D::<Color32>::new_aligned(
            prev_mipmap.width() / 2,
            prev_mipmap.height() / 2,
            CACHE_LINE_SIZE,
        );
        let curr_mipmap_chunks = curr_mipmap.as_positioned_chunks_mut(CHUNK_EXTENT, CHUNK_EXTENT);
        if curr_mipmap_chunks.len() < MIN_CHUNKS_FOR_PAR {
            curr_mipmap_chunks.into_iter().for_each(kernel);
        } else {
            curr_mipmap_chunks.into_par_iter().for_each(kernel);
        }

        curr_mipmap
    }
}
