use eframe::egui;
use eframe::egui::{
    Color32, ColorImage, TextureFilter, TextureHandle, TextureOptions, TextureWrapMode,
};
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::grid::{FrozenGrid, GridPoint, GridRect};
use ulam_leapers::simulation::PlayerId;
use ulam_leapers::util::pow2::{Pow2, floor_div};

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

#[derive(Default)]
pub struct GridRender {
    params: GridRenderParameters,
    handle: Option<TextureHandle>,
}

impl GridRender {
    pub fn handle(&self) -> &Option<TextureHandle> {
        &self.handle
    }

    // The caller to this function must guarantee that there are enough colors in
    // params.colors to facilitate every cell. Otherwise, the behavior is undefined.
    pub fn render_to_rgba_image(
        params: &GridRenderParameters,
        frozen_grid: &FrozenGrid<PlayerId>,
    ) -> ColorImage {
        match params.zoom {
            Magnification(_factor) => {
                let samples: Array2D<Color32> = frozen_grid
                    // We use sample_range2d_small_zoom_out_map_par with no minification
                    // because it's parallelized.
                    // Not actually faster in our current case ona a 1080p window,
                    // however it may be faster on larger displays or with differently shaped chunks.
                    // Should not be meaningfully slower in fast cases and will speed up slow cases.
                    .sample_range2d_small_zoom_out_map_par(
                        &params.bounds,
                        Pow2::new(1),
                        || Color32::from_rgb(0, 0, 0),
                        |acc, v| *acc = params.colors[v.index()],
                        |acc, _width, _height| acc,
                    );
                ColorImage::new(
                    [samples.width(), samples.height()],
                    samples.as_flat_slice().to_vec(),
                )
            }
            Minification(factor) => {
                // u32 is enough for 4096x4096 worst case
                // We do alpha too in case the compiler can vectorize it better than just rgb.
                #[repr(align(16))]
                struct AccCol {
                    r: u32,
                    g: u32,
                    b: u32,
                    a: u32,
                }
                let colors_u32 = params
                    .colors
                    .iter()
                    .map(|c| AccCol {
                        r: c.r() as u32,
                        g: c.g() as u32,
                        b: c.b() as u32,
                        a: c.a() as u32,
                    })
                    .collect::<Vec<_>>();
                let samples: Array2D<Color32> = frozen_grid.sample_range2d_small_zoom_out_map_par(
                    &params.bounds,
                    factor,
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
                );
                ColorImage::new(
                    [samples.width(), samples.height()],
                    samples.as_flat_slice().to_vec(),
                )
            }
        }
    }

    fn update(&mut self, ctx: &egui::Context, frozen_grid: &FrozenGrid<PlayerId>) {
        let texture_options = TextureOptions {
            magnification: TextureFilter::Nearest,
            minification: TextureFilter::Linear,
            wrap_mode: TextureWrapMode::ClampToEdge,
            mipmap_mode: None,
        };

        let image = Self::render_to_rgba_image(&self.params, &frozen_grid);
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
