use crate::gui::simulation_runner::SimulationRunner;
use crate::gui::subwindow::SubwindowResult::{Keep, Replace};
use crate::gui::subwindow::{Subwindow, SubwindowResult};
use crate::gui::widgets::leaper_attacks::LeaperAttacksView;
use crate::gui::widgets::player_relations::PlayerRelationsView;
use crate::gui::widgets::simulation_limits::{SimulationLimitsConstraints, SimulationLimitsInput};
use crate::gui::widgets::widget::StatefulWidget;
use eframe::egui;
use eframe::egui::{Context, ScrollArea, Ui, Vec2b};
use std::fs::File;
use std::path::PathBuf;
use ulam_leapers::game::persist::uls::UlsSimulation;
use ulam_leapers::game::simulation::{FinalizedSimulation, Game, Simulation};
use ulam_leapers::util::memory::MemSize;

const MIN_TURNS: u64 = 1_000_000;
const MAX_TURNS: u64 = 1_000_000 * 1_000_000;
const MIN_COMPLETE_SHELLS: u64 = 10;
const MAX_COMPLETE_SHELLS: u64 = 1_000_000;
const MIN_MEMORY_USAGE: MemSize = MemSize::gb(1);
const MAX_MEMORY_USAGE: MemSize = MemSize::tb(4);

pub struct SimulationResumer {
    finalized_simulation: FinalizedSimulation,

    simulation_limits_input: SimulationLimitsInput,
}

impl SimulationResumer {
    fn make_simulation_limits_constraints() -> SimulationLimitsConstraints {
        SimulationLimitsConstraints {
            memory_usage: MIN_MEMORY_USAGE..=MAX_MEMORY_USAGE,
            complete_shells: MIN_COMPLETE_SHELLS..=MAX_COMPLETE_SHELLS,
            turns: MIN_TURNS..=MAX_TURNS,
        }
    }

    pub fn load_from_file(path: PathBuf) -> std::io::Result<Self> {
        let file = File::open(path.clone())?;
        let mut reader = std::io::BufReader::new(file);
        let uls_sim = UlsSimulation::read_from(&mut reader)?;
        let fin_sim = FinalizedSimulation::from(uls_sim);
        let mut simulation_limits_input =
            SimulationLimitsInput::new(Self::make_simulation_limits_constraints());
        simulation_limits_input
            .set_turns(fin_sim.complete_turns() * 2)
            .map_err(|e| {
                std::io::Error::other(format!(
                    "Simulation resumer does not support this simulation: {e}"
                ))
            })?;

        Ok(Self {
            finalized_simulation: fin_sim,

            simulation_limits_input,
        })
    }
}

impl Subwindow for SimulationResumer {
    fn name(&self) -> String {
        "Resumer".to_string()
    }

    fn ui(mut self: Box<Self>, ui: &mut Ui) -> SubwindowResult {
        let mut submit = false;

        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::Frame::default().show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    self.show_players_ui(ui);

                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            self.show_info_ui(ui);
                        });
                    });

                    ui.vertical(|ui| {
                        self.simulation_limits_input.ui(ui);

                        if ui.button("Resume").clicked() {
                            submit = true;
                        }
                    });
                });
            });
        });

        if submit {
            let sim = Simulation::from(self.finalized_simulation);
            let limits = self.simulation_limits_input.build_limits();
            let runner = SimulationRunner::new(sim, limits);
            Replace(Box::new(runner))
        } else {
            Keep(self)
        }
    }

    fn not_ui(self: Box<Self>, ctx: &Context) -> SubwindowResult {
        Keep(self)
    }
}

impl SimulationResumer {
    pub fn make_player_name(index: usize) -> String {
        format!("Player {}", index + 1)
    }

    fn show_players_ui(&mut self, ui: &mut Ui) {
        let players = self.finalized_simulation.players();

        ui.horizontal_top(|ui| {
            ui.group(|ui| {
                ScrollArea::new(Vec2b::new(false, true)).show(ui, |ui| {
                    ui.vertical(|ui| {
                        for (i, player) in players.iter().enumerate() {
                            let name = Self::make_player_name(i);
                            let mut widget = LeaperAttacksView::with_name(name, player.attacks());
                            widget.ui(ui);
                        }
                    });
                });
            });

            let mut relations_widget = PlayerRelationsView::new(players);
            relations_widget.ui(ui);
        });
    }

    fn show_info_ui(&mut self, ui: &mut Ui) {
        let turns = self.finalized_simulation.complete_turns();
        let complete_shells = self.finalized_simulation.complete_shells();
        let side_cells = complete_shells.max(1) as usize * 2 - 1;
        let cells = side_cells * side_cells;
        let chunks = self.finalized_simulation.chunk_count();
        let memory_usage = self.finalized_simulation.memory_usage();

        ui.label(format!("Turns: {}M", turns / 1000 / 1000));
        ui.label(format!("Complete shells: {}", complete_shells));
        ui.label(format!("Number of cells: {}M", cells / 1000 / 1000));
        ui.label(format!("Number of chunks: {}", chunks));
        ui.label(format!("Size in memory: {}", memory_usage.display().si()));
    }
}
