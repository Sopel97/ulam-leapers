use eframe::egui;
use eframe::egui::{Color32, ColorImage, TextureFilter, TextureHandle, TextureOptions, TextureWrapMode};
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::grid::{FrozenGrid, GridPoint, GridRect};
use ulam_leapers::simulation::PlayerId;
use ulam_leapers::util::pow2::{floor_div, Pow2};

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

    pub fn render_to_rgba_image(params: &GridRenderParameters, frozen_grid: &FrozenGrid<PlayerId>) -> ColorImage {
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
                        |v| params.colors[v[(0, 0)].index()],
                    );
                ColorImage::new(
                    [samples.width(), samples.height()],
                    samples.as_flat_slice().to_vec(),
                )
            }
            Minification(factor) => {
                let samples: Array2D<Color32> = frozen_grid.sample_range2d_small_zoom_out_map_par(
                    &params.bounds,
                    factor,
                    |block| {
                        // Crude area interpolation without gamma correction.
                        let mut r: i64 = 0;
                        let mut g: i64 = 0;
                        let mut b: i64 = 0;
                        for y in 0..block.height() {
                            for x in 0..block.width() {
                                // SAFETY: Explicitly iterating within bounds.
                                let color = unsafe {
                                    params.colors[block.get_unchecked(x, y).index()]
                                };
                                r += color.r() as i64;
                                g += color.g() as i64;
                                b += color.b() as i64;
                            }
                        }
                        let count = Pow2::new(block.width() * block.height());
                        Color32::from_rgb(
                            floor_div(r, count) as u8,
                            floor_div(g, count) as u8,
                            floor_div(b, count) as u8,
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