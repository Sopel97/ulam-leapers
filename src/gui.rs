use eframe::egui;
use eframe::egui::{ColorImage, Rect, TextureHandle, TextureOptions, Vec2};
use eframe::emath::pos2;
use eframe::epaint::Color32;
use eframe::wgpu::PresentMode;
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::grid::{GridPoint, GridRect, GridVector};
use ulam_leapers::piece::LeaperAttacks;
use ulam_leapers::simulation::{Simulation, SimulationLimits};
use ulam_leapers::util::pow2;
use ulam_leapers::util::pow2::Pow2;

pub fn run() -> eframe::Result<()> {
    let mut options = eframe::NativeOptions::default();
    options.wgpu_options.present_mode = PresentMode::AutoVsync;
    options.wgpu_options.desired_maximum_frame_latency = Some(1);
    options.vsync = true;

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
    sim.finalize();
    let elapsed = start.elapsed();

    let simulated_turns = sim.simulated_turns();
    let complete_shells = sim.complete_shells();
    let frozen_grid = sim.finalize_to_frozen_grid();
    let finalized_memory_usage = frozen_grid.memory_usage();
    println!(
        "Simulated {} turns in {:?}.\nComplete shells: {}.\nEstimated memory usage: {} MiB.\nFinal memory usage: {} MiB.",
        simulated_turns,
        elapsed,
        complete_shells,
        end_memory_usage / 1024 / 1024,
        finalized_memory_usage / 1024 / 1024
    );

    let mut grid_view_control = GridViewControl {
        min_zoom_pow2: -3,
        max_zoom_pow2: 0,
        ..Default::default()
    };

    let mut prev_size = Vec2::ZERO;
    let mut prev_zoom_pow2 = grid_view_control.min_zoom_pow2 - 1;
    let mut handle: Option<TextureHandle> = None;

    eframe::run_ui_native("Ulam Leapers Explorer", options, move |ui, _frame| {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let rect = ui.clip_rect(); // Full canvas
            let painter = ui.painter_at(rect);

            // background
            painter.rect_filled(rect, 0.0, egui::Color32::BLACK);

            let curr_size = rect.size();
            let curr_zoom_pow2 = grid_view_control.zoom_pow2;
            if curr_size != prev_size || curr_zoom_pow2 != prev_zoom_pow2 {
                let timer = std::time::Instant::now();

                if curr_zoom_pow2 == 0 {
                    let our_rect = GridRect::with_size(
                        GridPoint::new(
                            -rect.width() as i32 / 2,
                            -rect.height() as i32 / 2,
                        ),
                        rect.width() as i32,
                        rect.height() as i32,
                    );
                    let colors = [Color32::WHITE, Color32::BLACK, Color32::RED];

                    let samples: Array2D<Color32> = frozen_grid.sample_range2d_map(
                        &our_rect,
                        |v| colors[v.index()],
                    );
                    let image = ColorImage::new(
                        [samples.width(), samples.height()],
                        samples.as_flat_slice().to_vec(),
                    );
                    handle = Some(ui.load_texture("name", image, TextureOptions::NEAREST));
                } else if curr_zoom_pow2 < 0 {
                    let minification = Pow2::from_exponent((-curr_zoom_pow2) as usize);
                    let minification_i32: i32 = minification.into();

                    let our_rect = GridRect::with_size(
                        GridPoint::new(
                            -rect.width() as i32 / 2 * minification_i32,
                            -rect.height() as i32 / 2 * minification_i32,
                        ),
                        rect.width() as i32 * minification_i32,
                        rect.height() as i32 * minification_i32,
                    );
                    let colors = [Color32::WHITE, Color32::BLACK, Color32::RED];

                    let samples: Array2D<Color32> = frozen_grid.sample_range2d_small_zoom_out_map(
                        &our_rect,
                        minification,
                        |block| {
                            // Crude area interpolation without gamma correction.
                            let mut r: i64 = 0;
                            let mut g: i64 = 0;
                            let mut b: i64 = 0;
                            for y in 0..block.height() {
                                for x in 0..block.width() {
                                    let color = colors[block[(x, y)].index()];
                                    r += color.r() as i64;
                                    g += color.g() as i64;
                                    b += color.b() as i64;
                                }
                            }
                            let count = Pow2::new(block.width() * block.height());
                            Color32::from_rgb(
                                pow2::floor_div(r, count) as u8,
                                pow2::floor_div(g, count) as u8,
                                pow2::floor_div(b, count) as u8,
                            )
                        },
                    );
                    let image = ColorImage::new(
                        [samples.width(), samples.height()],
                        samples.as_flat_slice().to_vec(),
                    );
                    handle = Some(ui.load_texture("name", image, TextureOptions::NEAREST));
                }

                prev_size = curr_size;
                prev_zoom_pow2 = curr_zoom_pow2;

                let elapsed = timer.elapsed();

                println!(
                    "{}x -> {} {} in {:?}",
                    2f32.powf(curr_zoom_pow2 as f32),
                    curr_size.x,
                    curr_size.y,
                    elapsed
                );
            }

            if let Some(handle) = &handle {
                // y-flip via uv
                painter.image(
                    handle.id(),
                    rect,
                    Rect::from_min_max(pos2(0.0, 1.0), pos2(1.0, 0.0)),
                    Color32::WHITE,
                );
            }

            egui::Window::new("grid_view_control")
                .scroll(false)
                .resizable([false, false]) // resizable so we can shrink if the text edit grows
                .constrain_to(ui.available_rect_before_wrap())
                .show(ui, |ui| grid_view_control.ui(ui));
        });
    })
}

pub struct GridViewControl {
    min_zoom_pow2: i32,
    max_zoom_pow2: i32,

    zoom_pow2: i32,
}

impl Default for GridViewControl {
    fn default() -> Self {
        GridViewControl {
            min_zoom_pow2: 0,
            max_zoom_pow2: 0,
            zoom_pow2: 0,
        }
    }
}

impl GridViewControl {
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Controls");
        ui.add(egui::Slider::new(&mut self.zoom_pow2, self.min_zoom_pow2..=self.max_zoom_pow2)
            .custom_formatter(|n, _| {
                let n = n as i32;
                if n >= 0 {
                    format!("{}x", 1 << n)
                } else {
                    format!("1/{}x", 1 << -n)
                }
            }));
    }
}