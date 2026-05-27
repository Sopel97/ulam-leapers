use eframe::egui;
use eframe::egui::{ColorImage, Rect, TextureHandle, TextureOptions, Vec2};
use eframe::emath::pos2;
use eframe::epaint::Color32;
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::grid::{GridPoint, GridRect, GridVector};
use ulam_leapers::piece::LeaperAttacks;
use ulam_leapers::simulation::{Simulation, SimulationLimits};
use ulam_leapers::util::pow2::Pow2;

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();

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

    let mut prev_size = Vec2::ZERO;
    let mut handle: Option<TextureHandle> = None;

    eframe::run_ui_native("Ulam Leapers Explorer", options, move |ui, _frame| {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let rect = ui.clip_rect(); // Full canvas
            let painter = ui.painter_at(rect);

            // background
            painter.rect_filled(rect, 0.0, egui::Color32::BLACK);

            let curr_size = rect.size();
            let minification = Pow2::new(8);
            let minification_i32: i32 = minification.into();
            if curr_size != prev_size {
                let our_rect = GridRect::with_size(
                    GridPoint::new(
                        -rect.width() as i32 / 2 * minification_i32,
                        -rect.height() as i32 / 2 * minification_i32,
                    ),
                    rect.width() as i32 * minification_i32,
                    rect.height() as i32 * minification_i32,
                );
                let colors = [Color32::WHITE, Color32::BLACK, Color32::RED];

                let timer = std::time::Instant::now();

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
                        let count = (block.width() * block.height()) as i64;
                        Color32::from_rgb((r / count) as u8, (g / count) as u8, (b / count) as u8)
                    },
                );
                let image = ColorImage::new(
                    [samples.width(), samples.height()],
                    samples.as_flat_slice().to_vec(),
                );
                handle = Some(ui.load_texture("name", image, TextureOptions::NEAREST));
                prev_size = curr_size;

                let elapsed = timer.elapsed();

                println!("{} {} -> {} {} in {:?}", rect.width(), rect.height(), samples.width(), samples.height(), elapsed);
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
        });
    })
}
