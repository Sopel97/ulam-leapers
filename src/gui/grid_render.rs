use eframe::egui;
use eframe::egui::{
    Color32, ColorImage, TextureFilter, TextureHandle, TextureOptions, TextureWrapMode,
};
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use std::collections::BTreeMap;
use ulam_leapers::collections::array2d::{Array2D, MutSlice2D};
use ulam_leapers::grid::{FrozenGrid, GridPoint, GridRect};
use ulam_leapers::simulation::PlayerId;
use ulam_leapers::util::align::CACHE_LINE_SIZE;
use ulam_leapers::util::pow2;
use ulam_leapers::util::pow2::{Pow2, ceil_to_multiple, floor_div, floor_mod, floor_to_multiple};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    Magnification(Pow2),
    Minification(Pow2),
}
use Zoom::*;

#[derive(Clone, PartialEq)]
pub struct GridRenderParameters {
    bounds: GridRect,
    colors: Vec<Color32>,
    zoom: Zoom,
}

impl GridRenderParameters {
    pub fn new(bounds: GridRect, colors: Vec<Color32>, zoom: Zoom) -> Self {
        Self {
            bounds,
            colors,
            zoom,
        }
    }

    pub fn bounds(&self) -> GridRect {
        self.bounds
    }
}

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

impl Default for GridRenderParameters {
    fn default() -> Self {
        GridRenderParameters {
            bounds: GridRect::with_size(GridPoint::new(0, 0), 0, 0),
            colors: default_player_colors()[..1].to_vec(),
            zoom: Magnification(Pow2::new(1)),
        }
    }
}

pub struct GridRenderMipMaps {
    bounds: GridRect,
    mipmaps_by_minification_factor: BTreeMap<Pow2, Array2D<Color32>>,
}

impl GridRenderMipMaps {
    pub fn handles_minification(&self, factor: Pow2) -> bool {
        self.mipmaps_by_minification_factor.contains_key(&factor)
    }

    pub fn estimate_memory_requirement(frozen_grid: &FrozenGrid<PlayerId>, lowest_minification: Pow2,
                                       highest_minification: Pow2,) -> usize {
        let mut grid_bounds = frozen_grid.bounds();
        grid_bounds.start.x = floor_to_multiple(grid_bounds.start.x, highest_minification);
        grid_bounds.start.y = floor_to_multiple(grid_bounds.start.y, highest_minification);
        grid_bounds.end.x = ceil_to_multiple(grid_bounds.end.x, highest_minification);
        grid_bounds.end.y = ceil_to_multiple(grid_bounds.end.y, highest_minification);
        
        let pixels_at_no_minification = grid_bounds.width() as usize * grid_bounds.height() as usize;
        let pixels_at_lowest_minification = floor_div(
            floor_div(pixels_at_no_minification, lowest_minification),
            lowest_minification,
        );

        // 1 + 1/4 + 1/16 + 1/64 + ... converges to 4/3
        pixels_at_lowest_minification * 4 * size_of::<Color32>() / 3
    }

    pub fn new(
        frozen_grid: &FrozenGrid<PlayerId>,
        colors: Vec<Color32>,
        lowest_minification: Pow2,
        highest_minification: Pow2,
    ) -> Self {
        // We allow a single level, but not less than that.
        assert!(lowest_minification <= highest_minification);

        // Strictly speaking we only require the width and height to be aligned to
        // the higher minification factor, but having this constraint on the whole rect
        // is very beneficial for how the Grid behaves.
        let mut grid_bounds = frozen_grid.bounds();
        grid_bounds.start.x = floor_to_multiple(grid_bounds.start.x, highest_minification);
        grid_bounds.start.y = floor_to_multiple(grid_bounds.start.y, highest_minification);
        grid_bounds.end.x = ceil_to_multiple(grid_bounds.end.x, highest_minification);
        grid_bounds.end.y = ceil_to_multiple(grid_bounds.end.y, highest_minification);

        // Disallow zero-sized bounds
        assert!(grid_bounds.width() > 0);
        assert!(grid_bounds.height() > 0);

        let mut slf = Self {
            bounds: grid_bounds,
            mipmaps_by_minification_factor: BTreeMap::new(),
        };

        slf.mipmaps_by_minification_factor.insert(
            lowest_minification,
            Self::make_mipmap_from_scratch(
                frozen_grid,
                grid_bounds,
                colors.clone(),
                lowest_minification,
            ),
        );
        let mut prev_minification = lowest_minification;
        let mut curr_minification = lowest_minification.next();
        while curr_minification <= highest_minification {
            // We know it exists because we put it either during the init or during the previous iteration.
            let prev_mipmap = slf
                .mipmaps_by_minification_factor
                .get(&prev_minification)
                .unwrap();

            slf.mipmaps_by_minification_factor
                .insert(curr_minification, Self::reduce_mipmap_2x(prev_mipmap));

            prev_minification = curr_minification;
            curr_minification = curr_minification.next();
        }

        slf
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

    // Private function, parameters are assumed to be correct.
    fn make_mipmap_from_scratch(
        frozen_grid: &FrozenGrid<PlayerId>,
        bounds: GridRect,
        colors: Vec<Color32>,
        minification: Pow2,
    ) -> Array2D<Color32> {
        GridRender::render_to_rgba_samples(
            &GridRenderParameters::new(bounds, colors, Minification(minification)),
            frozen_grid,
            &None,
        )
    }
}

// TODO: Refactor this because it's a mess now. Ideally make it borrow FrozenGrid.
//       GridRenderMipMaps should perhaps become a part of this too.
//       Make it NOT hold the TextureHandle.
#[derive(Default)]
pub struct GridRender {
    params: GridRenderParameters,
    handle: Option<TextureHandle>,
    mipmaps: Option<GridRenderMipMaps>,
}

impl GridRender {
    pub fn handle(&self) -> &Option<TextureHandle> {
        &self.handle
    }

    pub fn mipmap_bounds(&self) -> Option<GridRect> {
        self.mipmaps.as_ref().map(|m| m.bounds)
    }

    pub fn highest_mipmap_minification_factor(&self) -> Option<Pow2> {
        self.mipmaps
            .as_ref()?
            .mipmaps_by_minification_factor
            .keys()
            .max()
            .copied()
    }

    pub fn generate_mipmaps(
        &mut self,
        frozen_grid: &FrozenGrid<PlayerId>,
        colors: Vec<Color32>,
        lowest_minification: Pow2,
        highest_minification: Pow2,
    ) {
        self.mipmaps = Some(GridRenderMipMaps::new(
            frozen_grid,
            colors,
            lowest_minification,
            highest_minification,
        ));
    }

    fn render_to_rgba_samples_for_minification_direct(
        frozen_grid: &FrozenGrid<PlayerId>,
        bounds: GridRect,
        colors: &[Color32],
        factor: Pow2,
    ) -> Array2D<Color32> {
        // u32 is enough for 4096x4096 worst case
        // We do alpha too in case the compiler can vectorize it better than just rgb.
        #[repr(align(16))]
        struct AccCol {
            r: u32,
            g: u32,
            b: u32,
            a: u32,
        }
        let colors_u32 = colors
            .iter()
            .map(|c| AccCol {
                r: c.r() as u32,
                g: c.g() as u32,
                b: c.b() as u32,
                a: c.a() as u32,
            })
            .collect::<Vec<_>>();
        frozen_grid.sample_range2d_small_zoom_out_map_par(
            &bounds,
            factor,
            colors[0],
            || AccCol {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            |acc, v| {
                // SAFETY: We can't guarantee the safety here,
                //         the caller must make sure there is enough colors.
                //         However, this is a hot loop, we need it for speed.
                let color = unsafe { colors_u32.get_unchecked(v.index()) };
                acc.r += color.r;
                acc.g += color.g;
                acc.b += color.b;
                acc.a += color.a;
            },
            |acc, width, height| {
                let count = Pow2::new(width * height);
                Color32::from_rgb(
                    floor_div(acc.r, count) as u8,
                    floor_div(acc.g, count) as u8,
                    floor_div(acc.b, count) as u8,
                )
            },
        )
    }

    fn render_to_rgba_samples_for_minification_using_mipmaps(
        bounds: GridRect,
        factor: Pow2,
        mipmaps: &GridRenderMipMaps,
    ) -> Array2D<Color32> {
        if !bounds.is_aligned_to_pow2(factor) {
            panic!("Region is not aligned to the minification factor.");
        }

        if !mipmaps.handles_minification(factor) {
            panic!("Mipmaps cannot handle this minification factor.");
        }

        assert!(mipmaps.bounds.is_aligned_to_pow2(factor));

        // We convert the bounds into local coordinates (after minification).
        let mut src_bounds = mipmaps.bounds;
        src_bounds.start.x = floor_div(src_bounds.start.x, factor);
        src_bounds.start.y = floor_div(src_bounds.start.y, factor);
        src_bounds.end.x = floor_div(src_bounds.end.x, factor);
        src_bounds.end.y = floor_div(src_bounds.end.y, factor);

        let mut dst_bounds = bounds;
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
        let mipmap = mipmaps.mipmaps_by_minification_factor.get(&factor).unwrap();

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

    pub fn render_to_rgba_samples(
        params: &GridRenderParameters,
        frozen_grid: &FrozenGrid<PlayerId>,
        mipmaps: &Option<GridRenderMipMaps>,
    ) -> Array2D<Color32> {
        match params.zoom {
            Magnification(_factor) => {
                frozen_grid
                    // We use sample_range2d_small_zoom_out_map_par with no minification
                    // because it's parallelized.
                    // Not actually faster in our current case ona a 1080p window,
                    // however it may be faster on larger displays or with differently shaped chunks.
                    // Should not be meaningfully slower in fast cases and will speed up slow cases.
                    .sample_range2d_small_zoom_out_map_par(
                        &params.bounds,
                        Pow2::new(1),
                        params.colors[0],
                        || Color32::from_rgb(0, 0, 0),
                        |acc, v| *acc = params.colors[v.index()],
                        |acc, _width, _height| acc,
                    )
            }
            Minification(factor) => {
                if let Some(mipmaps) = mipmaps
                    && mipmaps.handles_minification(factor)
                {
                    Self::render_to_rgba_samples_for_minification_using_mipmaps(
                        params.bounds,
                        factor,
                        mipmaps,
                    )
                } else {
                    Self::render_to_rgba_samples_for_minification_direct(
                        frozen_grid,
                        params.bounds,
                        &params.colors,
                        factor,
                    )
                }
            }
        }
    }

    // The caller to this function must guarantee that there are enough colors in
    // params.colors to facilitate every cell. Otherwise, the behavior is undefined.
    pub fn render_to_rgba_image(
        params: &GridRenderParameters,
        frozen_grid: &FrozenGrid<PlayerId>,
        mipmaps: &Option<GridRenderMipMaps>,
    ) -> ColorImage {
        let samples = Self::render_to_rgba_samples(params, frozen_grid, mipmaps);
        ColorImage::new(
            [samples.width(), samples.height()],
            samples.as_flat_slice().to_vec(),
        )
    }

    fn update(&mut self, ctx: &egui::Context, frozen_grid: &FrozenGrid<PlayerId>) {
        let texture_options = TextureOptions {
            magnification: TextureFilter::Nearest,
            minification: TextureFilter::Linear,
            wrap_mode: TextureWrapMode::ClampToEdge,
            mipmap_mode: None,
        };

        let image = Self::render_to_rgba_image(&self.params, frozen_grid, &self.mipmaps);
        self.handle = Some(ctx.load_texture("name", image, texture_options));
    }

    // Returns true if an update was actually performed (needed), false otherwise.
    pub fn maybe_update(
        &mut self,
        ctx: &egui::Context,
        frozen_grid: &FrozenGrid<PlayerId>,
        new_params: GridRenderParameters,
    ) -> bool {
        if self.params != new_params {
            self.params = new_params;
            self.update(ctx, frozen_grid);
            true
        } else {
            false
        }
    }
}
