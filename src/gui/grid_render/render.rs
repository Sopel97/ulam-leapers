use crate::gui::grid_render::samplers::{AvgMapColor32Collector, MapLastCollector};
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
use ulam_leapers::game::grid::FrozenGrid;
use ulam_leapers::game::sampler::{FrozenGridSampler, SamplerProgress};
use ulam_leapers::game::simulation::{FinalizedSimulation, PlayerId};
use ulam_leapers::math::color::Color32Accumulator;
use ulam_leapers::math::pow2::{Pow2, div_floor};
use ulam_leapers::math::rect::GridRect;
use ulam_leapers::math::zoom::Zoom;
use ulam_leapers::util::align::CACHE_LINE_SIZE;
use ulam_leapers::util::blit::{Blit2D, blit_array2d};
use ulam_leapers::util::cache::LockStepCache;
use ulam_leapers::util::cancel::{Canceled, CancellationToken};
use ulam_leapers::util::memory::MemSize;
use ulam_leapers::util::sync::DeferredValue;

const PREDEFINED_COLORS: [Color32; 9] = [
    Color32::WHITE,
    Color32::BLACK,
    Color32::RED,
    Color32::BLUE,
    Color32::YELLOW,
    Color32::GREEN,
    Color32::CYAN,
    Color32::MAGENTA,
    Color32::BROWN,
];

pub fn default_player_colors(max_id: PlayerId) -> Vec<Color32> {
    let color_count = max_id.index() + 1;

    if color_count <= PREDEFINED_COLORS.len() {
        PREDEFINED_COLORS[0..color_count].to_vec()
    } else {
        let mut colors = Vec::with_capacity(color_count);
        colors.extend_from_slice(&PREDEFINED_COLORS);
        for i in PREDEFINED_COLORS.len()..color_count {
            let c = i as u8;
            colors.push(Color32::from_rgb(c, c, c));
        }
        colors
    }
}

type CacheType = LockStepCache<(ChunkOrigin, Pow2), Array2D<Color32>>;
type MipmapStorageType = BTreeMap<Pow2, Array2D<Color32>>;

#[derive(Debug)]
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

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum MipmapGenerationProgress {
    LargestMipmap { chunks_done: u64, chunks_total: u64 },
    SmallerMipmap { zoom: Zoom<Pow2> },
}

pub struct GridRender {
    texture: TextureHandle,

    world_bounds: GridRect,
    zoom: Zoom<Pow2>,
    colors: Vec<Color32>,
}

impl GridRender {
    pub fn texture(&self) -> &TextureHandle {
        &self.texture
    }

    pub fn world_bounds(&self) -> GridRect {
        self.world_bounds
    }

    pub fn zoom(&self) -> Zoom<Pow2> {
        self.zoom
    }

    pub fn colors(&self) -> &[Color32] {
        &self.colors
    }

    pub fn is_outdated(
        &self,
        renderer: &GridRenderer,
        world_bounds: GridRect,
        zoom: Zoom<Pow2>,
    ) -> bool {
        zoom != self.zoom || world_bounds != self.world_bounds || renderer.colors != self.colors
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
            self.can_set_colors(),
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
        self.mipmaps
            .get()?
            .by_minification_factor
            .keys()
            .max()
            .copied()
    }

    fn make_sampler_for_minification(
        &self,
        world_bounds: GridRect,
        factor: Pow2,
    ) -> FrozenGridSampler<'_, PlayerId, AvgMapColor32Collector> {
        FrozenGridSampler::new_with_minification(
            &self.grid,
            world_bounds,
            factor,
            self.colors[0],
            AvgMapColor32Collector::new(self.colors.as_slice()),
        )
    }

    fn render_to_rgba_samples_for_minification_direct(
        &self,
        world_bounds: GridRect,
        factor: Pow2,
    ) -> Array2D<Color32> {
        let sampler = self.make_sampler_for_minification(world_bounds, factor);

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
        world_bounds: GridRect,
        factor: Pow2,
    ) -> Array2D<Color32> {
        if !world_bounds.is_aligned_to_pow2(factor) {
            panic!("Region is not aligned to the minification factor.");
        }

        if !self.has_mipmap(factor) {
            panic!("Mipmaps cannot handle this minification factor.");
        }

        let mipmap_bounds = self.mipmap_bounds();

        assert!(mipmap_bounds.is_aligned_to_pow2(factor));

        // We convert the bounds into local coordinates (after minification).
        let src_bounds = mipmap_bounds.map_coords(|c| div_floor(c, factor));
        let dst_bounds = world_bounds.map_coords(|c| div_floor(c, factor));

        let mut res = Array2D::<Color32>::new_aligned(
            dst_bounds.width() as usize,
            dst_bounds.height() as usize,
            CACHE_LINE_SIZE,
        );

        // We could maybe make this lock shorter but who cares. This struct is not supposed
        // to be used concurrently anyway.
        let mipmaps = &self.mipmaps.get().unwrap().by_minification_factor;
        let mipmap = mipmaps.get(&factor).unwrap();

        // The intersection of the whole mipmap and the view is what we actually need to blit.
        let intersection = src_bounds.intersection(&dst_bounds).unwrap();
        let src = intersection.start - src_bounds.start;
        let dst = intersection.start - dst_bounds.start;

        blit_array2d(
            mipmap,
            &mut res,
            &Blit2D {
                src_x: src.x as usize,
                src_y: src.y as usize,
                dst_x: dst.x as usize,
                dst_y: dst.y as usize,
                width: intersection.width() as usize,
                height: intersection.height() as usize,
            },
        );

        res
    }

    pub fn render_to_rgba_samples(
        &self,
        world_bounds: GridRect,
        zoom: Zoom<Pow2>,
    ) -> Array2D<Color32> {
        match zoom {
            Zoom::Magnification(_factor) => {
                let colors = &self.colors;
                let sampler = FrozenGridSampler::new(
                    &self.grid,
                    world_bounds,
                    colors[0],
                    MapLastCollector::new(colors),
                );
                // Do not use a cache for magnification because it's cheap to
                // compute and the results take more memory than the probed chunk cells.
                sampler.par_sample()
            }
            Zoom::Minification(factor) => {
                if self.has_mipmap(factor) {
                    self.render_to_rgba_samples_for_minification_using_mipmaps(world_bounds, factor)
                } else {
                    self.render_to_rgba_samples_for_minification_direct(world_bounds, factor)
                }
            }
        }
    }

    // The caller to this function must guarantee that there are enough colors in
    // params.colors to facilitate every cell. Otherwise, the behavior is undefined.
    pub fn render_to_rgba_image(&self, world_bounds: GridRect, zoom: Zoom<Pow2>) -> ColorImage {
        let samples = self.render_to_rgba_samples(world_bounds, zoom);
        ColorImage::new(
            [samples.width(), samples.height()],
            samples.as_flat_slice().to_vec(),
        )
    }

    pub fn render_texture(
        &mut self,
        ctx: &egui::Context,
        world_bounds: GridRect,
        zoom: Zoom<Pow2>,
    ) -> GridRender {
        let texture_options = TextureOptions {
            magnification: TextureFilter::Nearest,
            minification: TextureFilter::Linear,
            wrap_mode: TextureWrapMode::ClampToEdge,
            mipmap_mode: None,
        };

        let image = self.render_to_rgba_image(world_bounds, zoom);
        let texture = ctx.load_texture("name", image, texture_options);
        GridRender {
            texture,
            world_bounds,
            zoom,
            colors: self.colors.clone(),
        }
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
        let grid_bounds = self
            .grid
            .bounds()
            .hull_aligned_to_pow2(highest_minification);

        let pixels_at_no_minification =
            grid_bounds.width() as usize * grid_bounds.height() as usize;
        // lowest_minification squared we're reducing area
        let pixels_at_lowest_minification = div_floor(
            pixels_at_no_minification,
            lowest_minification * lowest_minification,
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

    pub fn generate_mipmaps_async(
        &mut self,
        lowest_minification: Pow2,
        highest_minification: Pow2,
    ) -> Arc<Mutex<MipmapGenerationProgress>> {
        // We allow a single level, but not less than that.
        assert!(lowest_minification <= highest_minification);

        // Strictly speaking we only require the width and height to be aligned to
        // the higher minification factor, but having this constraint on the whole rect
        // is very beneficial for how the Grid behaves.
        let grid_bounds = self
            .grid
            .bounds()
            .hull_aligned_to_pow2(highest_minification);

        // Disallow zero-sized bounds
        assert!(grid_bounds.width() > 0);
        assert!(grid_bounds.height() > 0);

        let is_finished = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(MipmapGenerationProgress::LargestMipmap {
            chunks_done: 0,
            chunks_total: 0,
        }));
        let progress_clone = Arc::clone(&progress);
        let progress_callback = move |p: SamplerProgress| {
            *progress_clone.lock().unwrap() = MipmapGenerationProgress::LargestMipmap {
                chunks_done: p.done,
                chunks_total: p.total,
            };
        };

        let grid_ref = Arc::clone(&self.grid);
        let default_color = self.colors[0];
        let collector = AvgMapColor32Collector::new(self.colors.as_slice());

        let is_finished_clone = Arc::clone(&is_finished);
        let job = move |ct: CancellationToken| {
            let mut mipmaps = MipmapStorageType::new();

            let mut sampler = FrozenGridSampler::new_with_minification(
                grid_ref.as_ref(),
                grid_bounds,
                lowest_minification,
                default_color,
                collector,
            );
            // This is a long-running task, and it may interfere with other sampling jobs,
            // don't rely on rayon to properly load-balance this (it usually doesn't)
            sampler.use_dedicated_thread_pool(rayon::current_num_threads());
            let master_mipmap = sampler.par_sample_cancellable(ct.clone(), progress_callback);
            if master_mipmap.is_none() {
                return Err(Canceled);
            }

            mipmaps.insert(lowest_minification, master_mipmap.unwrap());

            // IMPLEMENTATION NOTE:
            // We reduce mipmaps iteratively as a whole, by 2x each iteration.
            // This introduces some accumulated error as we repeatedly convert between
            // 8-bit SRGB and 16-bit linear color.
            // Ideally we would reduce each chunk pyramid at once to allow maintaining higher
            // precision accumulators and round only once for each mipmap level,
            // however, this is exceedingly hard to parallelize in safe rust.
            // It would require a `Array2D::as_positioned_chunks_mut` alternative spanning
            // the whole pyramid of mipmaps. It would also require temporary accumulators
            // that should somehow be thread local to minimize allocations.
            // Way too much hassle for little gain, so for now we eat the loss of accuracy.
            let mut prev_minification = lowest_minification;
            let mut curr_minification = lowest_minification.next();
            while curr_minification <= highest_minification {
                if ct.is_canceled() {
                    return Err(Canceled);
                }

                // We know it exists because we put it either during the init or during the previous iteration.
                let prev_mipmap = mipmaps.get(&prev_minification).unwrap();

                mipmaps.insert(curr_minification, Self::reduce_mipmap_2x(prev_mipmap));

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

        progress
    }

    fn reduce_mipmap_2x(prev_mipmap: &Array2D<Color32>) -> Array2D<Color32> {
        const CHUNK_EXTENT: usize = 1024;
        const MIN_CHUNKS_FOR_PAR: usize = 8; // NOTE: untuned, just guess

        assert!(prev_mipmap.width().is_multiple_of(2));
        assert!(prev_mipmap.height().is_multiple_of(2));

        let lerp4 = |c0: Color32, c1: Color32, c2: Color32, c3: Color32| {
            let mut acc = Color32Accumulator::from_srgb(c0);
            acc += Color32Accumulator::from_srgb(c1);
            acc += Color32Accumulator::from_srgb(c2);
            acc += Color32Accumulator::from_srgb(c3);
            acc.average_to_srgb_pow2_count(Pow2::from_exponent(2))
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
