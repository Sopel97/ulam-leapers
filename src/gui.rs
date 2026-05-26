use eframe::egui;
use eframe::egui::{ColorImage, Rect, TextureHandle, TextureOptions, Vec2};
use eframe::emath::pos2;
use eframe::epaint::Color32;
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::grid::{GridPoint, GridRect, GridVector};
use ulam_leapers::piece::LeaperAttacks;
use ulam_leapers::simulation::{Simulation, SimulationLimits};

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();

    let mut sim = Simulation::new();
    let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
    let p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
    sim.add_player_enemy(p1, p2);
    sim.add_player_enemy(p2, p1);

    let start = std::time::Instant::now();
    let _ = sim.simulate(SimulationLimits::new().with_turn_limit(100_000_000).with_memory_limit(32 * 1024 * 1024 * 1024));
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
            if curr_size != prev_size {
                let our_rect = GridRect::with_size(
                    GridPoint::new(-rect.width() as i32 / 2, -rect.height() as i32 / 2),
                    rect.width() as i32,
                    rect.height() as i32
                );
                let colors = [Color32::WHITE, Color32::BLACK, Color32::RED];
                let samples: Array2D<Color32> = frozen_grid.sample_range2d_map(&our_rect, |v| colors[v.index()]);
                let image = ColorImage::new([samples.width(), samples.height()], samples.as_flat_slice().to_vec());
                handle = Some(ui.load_texture("name", image, TextureOptions::NEAREST));
                prev_size = curr_size;
                println!("{} {}", samples.width(), samples.height());
            }

            if let Some(handle) = &handle {
                // y-flip via uv
                painter.image(handle.id(), rect, Rect::from_min_max(pos2(0.0, 1.0), pos2(1.0, 0.0)), Color32::WHITE);
            }
        });
    })
}