use eframe::egui;
use eframe::egui::{ColorImage, Rect, Sense, TextureHandle, TextureOptions, Ui};
use eframe::emath::pos2;
use eframe::epaint::Color32;
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::grid::{FrozenGrid, GridPoint, GridRect, GridVector};
use ulam_leapers::io::{WriteTo, ReadFrom};
use ulam_leapers::piece::LeaperAttacks;
use ulam_leapers::simulation::{FinalizedSimulation, Game, PlayerId, Simulation, SimulationLimits};
use ulam_leapers::util::pow2::{Pow2, floor_div, floor_to_multiple};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Zoom {
    Magnification(Pow2),
    Minification(Pow2),
}

use Zoom::*;
use crate::gui::Subwindow;

#[derive(Clone, PartialEq)]
struct GridRenderParameters {
    bounds: GridRect,
    colors: Vec<Color32>,
    zoom: Zoom,
}

fn default_player_colors() -> Vec<Color32> {
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
            zoom: Zoom::Magnification(Pow2::new(1)),
        }
    }
}

struct GridRender {
    params: GridRenderParameters,
    handle: Option<TextureHandle>,
}

impl Default for GridRender {
    fn default() -> Self {
        GridRender {
            params: GridRenderParameters::default(),
            handle: None,
        }
    }
}

impl GridRender {
    fn controls_to_params(
        controls: &GridViewControls,
        viewport_width: usize,
        viewport_height: usize,
    ) -> GridRenderParameters {
        let zoom = match controls.zoom_pow2 {
            e @ 0.. => Magnification(Pow2::from_exponent(e as usize)),
            e @ ..0 => Minification(Pow2::from_exponent((-e) as usize)),
        };

        let bounds = match zoom {
            Magnification(factor) => {
                let origin_x = controls.origin_x as i32;
                let origin_y = controls.origin_y as i32;

                GridRect::with_size(
                    GridPoint::new(
                        origin_x - floor_div(viewport_width / 2, factor) as i32,
                        origin_y - floor_div(viewport_height / 2, factor) as i32,
                    ),
                    floor_div(viewport_width as i32, factor),
                    floor_div(viewport_height as i32, factor),
                )
            }
            Minification(factor) => {
                // We have to ensure proper alignment for the sampling.
                let origin_x = floor_to_multiple(controls.origin_x as i32, factor);
                let origin_y = floor_to_multiple(controls.origin_y as i32, factor);
                let factor_i32: i32 = factor.into();

                GridRect::with_size(
                    GridPoint::new(
                        floor_to_multiple(origin_x, factor)
                            - viewport_width as i32 / 2 * factor_i32,
                        floor_to_multiple(origin_y, factor)
                            - viewport_height as i32 / 2 * factor_i32,
                    ),
                    viewport_width as i32 * factor_i32,
                    viewport_height as i32 * factor_i32,
                )
            }
        };

        GridRenderParameters {
            bounds,
            colors: controls.player_colors.clone(),
            zoom,
        }
    }

    fn update(&mut self, ui: &Ui, frozen_grid: &FrozenGrid<PlayerId>) {
        match self.params.zoom {
            Magnification(_factor) => {
                let samples: Array2D<Color32> = frozen_grid
                    // We use sample_range2d_small_zoom_out_map_par with no minification
                    // because it's parallelized.
                    // Not actually faster in our current case ona a 1080p window,
                    // however it may be faster on larger displays or with differently shaped chunks.
                    // Should not be meaningfully slower in fast cases and will speed up slow cases.
                    .sample_range2d_small_zoom_out_map_par(
                        &self.params.bounds,
                        Pow2::new(1),
                        |v| self.params.colors[v[(0, 0)].index()]);
                let image = ColorImage::new(
                    [samples.width(), samples.height()],
                    samples.as_flat_slice().to_vec(),
                );
                self.handle = Some(ui.load_texture("name", image, TextureOptions::NEAREST));
            }
            Minification(factor) => {
                let samples: Array2D<Color32> = frozen_grid.sample_range2d_small_zoom_out_map_par(
                    &self.params.bounds,
                    factor,
                    |block| {
                        // Crude area interpolation without gamma correction.
                        let mut r: i64 = 0;
                        let mut g: i64 = 0;
                        let mut b: i64 = 0;
                        for y in 0..block.height() {
                            for x in 0..block.width() {
                                // SAFETY: Explicitly iterating within bounds.
                                let color = unsafe { self.params.colors[block.get_unchecked(x, y).index()] };
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
                let image = ColorImage::new(
                    [samples.width(), samples.height()],
                    samples.as_flat_slice().to_vec(),
                );
                self.handle = Some(ui.load_texture("name", image, TextureOptions::NEAREST));
            }
        }
    }

    // Returns true if an update was actually performed (needed), false otherwise.
    pub fn maybe_update(
        &mut self,
        ui: &Ui,
        frozen_grid: &FrozenGrid<PlayerId>,
        controls: &GridViewControls,
        viewport_width: usize,
        viewport_height: usize,
    ) -> bool {
        let new_params = Self::controls_to_params(controls, viewport_width, viewport_height);
        if self.params != new_params {
            self.params = new_params;
            self.update(ui, frozen_grid);
            true
        } else {
            false
        }
    }
}

pub struct GridExplorer {
    grid_view_controls: GridViewControls,
    finalized_sim: FinalizedSimulation,
    grid_render: GridRender,
}

impl Subwindow for GridExplorer {
    fn name(&self) -> String {
        "Explorer".to_owned()
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::Window::new("grid_view_control")
                .scroll(false)
                .resizable([false, false]) // resizable so we can shrink if the text edit grows
                .constrain_to(ui.available_rect_before_wrap())
                .show(ui, |ui| self.grid_view_controls.ui(ui));

            let rect = ui.clip_rect(); // Full canvas

            self.grid_view_controls.update_from_canvas_events(ui, &rect);

            let painter = ui.painter_at(rect);

            // background
            painter.rect_filled(rect, 0.0, egui::Color32::WHITE);

            let timer = std::time::Instant::now();
            let updated = self.grid_render.maybe_update(
                ui,
                self.finalized_sim.grid(),
                &self.grid_view_controls,
                rect.width() as usize,
                rect.height() as usize,
            );
            let elapsed = timer.elapsed();

            if updated {
                println!(
                    "{}x -> {} {} in {:?}",
                    2f32.powf(self.grid_view_controls.zoom_pow2 as f32),
                    rect.width() as usize,
                    rect.height() as usize,
                    elapsed
                );
            }

            if let Some(handle) = &self.grid_render.handle {
                // y-flip via uv
                painter.image(
                    handle.id(),
                    rect,
                    Rect::from_min_max(pos2(0.0, 1.0), pos2(1.0, 0.0)),
                    Color32::WHITE,
                );
            }
        });
    }
}

impl GridExplorer {
    pub fn new() -> Self {
        let mut sim = Simulation::new();
        let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        let p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        sim.add_player_enemy(p1, p2);
        sim.add_player_enemy(p2, p1);

        let start = std::time::Instant::now();
        let _ = sim.simulate(
            SimulationLimits::new()
                .with_turn_limit(100_000_000)
                .with_memory_limit(32 * 1024 * 1024 * 1024),
        );
        let end_memory_usage = sim.memory_usage();
        let finalized_sim = sim.finalize();
        let elapsed = start.elapsed();

        let simulated_turns = finalized_sim.simulated_turns();
        let complete_shells = finalized_sim.complete_shells();
        let player_count = finalized_sim.player_count();
        let finalized_memory_usage = finalized_sim.memory_usage();
        println!(
            "Simulated {} turns in {:?}.\nComplete shells: {}.\nEstimated memory usage: {} MiB.\nFinal memory usage: {} MiB.",
            simulated_turns,
            elapsed,
            complete_shells,
            end_memory_usage / 1024 / 1024,
            finalized_memory_usage / 1024 / 1024
        );

        let start = std::time::Instant::now();
        let mut serialized = Vec::<u8>::with_capacity(1024);
        finalized_sim.write_to(&mut serialized).unwrap();
        let finalized_sim = FinalizedSimulation::read_from(&mut serialized.as_slice()).unwrap();
        let elapsed = start.elapsed();
        println!("Serialize -> deserialize roundtrip in {:?}", elapsed);

        let grid_view_controls = GridViewControls {
            min_zoom_pow2: -3,
            max_zoom_pow2: 3,
            complete_shells: complete_shells.clone(),
            player_count: player_count.clone(),
            player_colors: default_player_colors()[..=player_count].to_vec(),
            ..Default::default()
        };

        let grid_render = GridRender::default();

        Self {
            grid_render,
            finalized_sim,
            grid_view_controls,
        }
    }
}

pub struct GridViewControls {
    min_zoom_pow2: i32,
    max_zoom_pow2: i32,
    complete_shells: i32,
    player_count: usize,
    player_colors: Vec<Color32>,

    zoom_pow2: i32,
    origin_x: f32,
    origin_y: f32,
}

impl Default for GridViewControls {
    fn default() -> Self {
        GridViewControls {
            min_zoom_pow2: 0,
            max_zoom_pow2: 0,
            complete_shells: 0,
            player_count: 0,
            player_colors: default_player_colors()[..1].to_vec(),

            zoom_pow2: 0,
            origin_x: 0f32,
            origin_y: 0f32,
        }
    }
}

impl GridViewControls {
    fn update_from_canvas_events(&mut self, ui: &mut Ui, rect: &Rect) {
        let response = ui.allocate_rect(*rect, Sense::drag() | Sense::hover());

        if response.hovered() {
            let mut new_zoom_pow2 = self.zoom_pow2;
            let middle_pos = (rect.min + rect.max.to_vec2()) * 0.5f32;
            let mouse = response
                .hover_pos()
                .map(|pos| {
                    // Invert y to match world coordinates.
                    pos2(pos.x, rect.height() - pos.y) - rect.min.to_vec2()
                })
                .unwrap_or(middle_pos)
                - middle_pos;

            ui.input(|i| {
                for event in &i.events {
                    if let egui::Event::MouseWheel { delta, .. } = event {
                        new_zoom_pow2 += delta.y as i32;
                    }
                }
            });

            new_zoom_pow2 = new_zoom_pow2.clamp(self.min_zoom_pow2, self.max_zoom_pow2);

            // Reproject with respect to the origin
            if new_zoom_pow2 != self.zoom_pow2 {
                let old_scale = (self.zoom_pow2 as f32).exp2();
                let new_scale = (new_zoom_pow2 as f32).exp2();

                let mouse_world_x = self.origin_x + mouse.x / old_scale;
                let mouse_world_y = self.origin_y + mouse.y / old_scale;

                self.origin_x = mouse_world_x - mouse.x / new_scale;
                self.origin_y = mouse_world_y - mouse.y / new_scale;

                self.zoom_pow2 = new_zoom_pow2;
            }
        }

        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            let zoom_scale = 0.5f32.powf(self.zoom_pow2 as f32);
            self.origin_x -= zoom_scale * delta.x;
            self.origin_y += zoom_scale * delta.y;
        }

        let bounds = self.complete_shells as f32;
        self.origin_x = self.origin_x.clamp(-bounds, bounds);
        self.origin_y = self.origin_y.clamp(-bounds, bounds);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Controls");
        ui.add(
            egui::Slider::new(&mut self.zoom_pow2, self.min_zoom_pow2..=self.max_zoom_pow2)
                .text("Zoom")
                .custom_formatter(|n, _| {
                    let n = n as i32;
                    if n >= 0 {
                        format!("{}x", 1 << n)
                    } else {
                        format!("1/{}x", 1 << -n)
                    }
                }),
        );

        // How many per pixel.
        let coord_drag_speed = (self.complete_shells / 200) as f64;
        ui.add(
            egui::Slider::new(
                &mut self.origin_x,
                -self.complete_shells as f32..=self.complete_shells as f32,
            )
                .text("X")
                .drag_value_speed(coord_drag_speed),
        );
        ui.add(
            egui::Slider::new(
                &mut self.origin_y,
                -self.complete_shells as f32..=self.complete_shells as f32,
            )
                .text("Y")
                .drag_value_speed(coord_drag_speed),
        );

        for player_id in 0..=self.player_count {
            ui.horizontal_wrapped(|ui| {
                ui.color_edit_button_srgba(&mut self.player_colors[player_id]);
                if player_id == 0 {
                    ui.label("Empty");
                } else {
                    ui.label(format!("Player {}", player_id));
                }
            });
            ui.end_row();
        }
    }
}
