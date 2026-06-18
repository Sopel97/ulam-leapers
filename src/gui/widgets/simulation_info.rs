use eframe::egui::Ui;
use ulam_leapers::game::simulation::{FinalizedSimulation, Game};

pub fn show_finalized_simulation_info_ui(finalized_simulation: &FinalizedSimulation, ui: &mut Ui) {
    let turns = finalized_simulation.complete_turns();
    let complete_shells = finalized_simulation.complete_shells();
    let side_cells = complete_shells.max(1) as usize * 2 - 1;
    let cells = side_cells * side_cells;
    let chunks = finalized_simulation.chunk_count();
    let memory_usage = finalized_simulation.memory_usage();

    ui.label(format!("Turns: {}M", turns / 1000 / 1000));
    ui.label(format!("Complete shells: {}", complete_shells));
    ui.label(format!("Number of cells: {}M", cells / 1000 / 1000));
    ui.label(format!("Number of chunks: {}", chunks));
    ui.label(format!("Size in memory: {}", memory_usage.display().si()));
}
