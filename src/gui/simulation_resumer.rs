use crate::gui::simulation_runner::SimulationRunner;
use crate::gui::subwindow::SubwindowResult::{Keep, Replace};
use crate::gui::subwindow::{Subwindow, SubwindowResult};
use crate::gui::util::{make_player_name, ContextOrUi};
use crate::gui::widgets::leaper_attacks::LeaperAttacksView;
use crate::gui::widgets::player_relations::PlayerRelationsView;
use crate::gui::widgets::simulation_limits::{SimulationLimitsConstraints, SimulationLimitsInput};
use crate::gui::widgets::widget::StatefulWidget;
use eframe::egui;
use eframe::egui::{Context, ScrollArea, Ui, Vec2b};
use std::fs::File;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{sleep, JoinHandle};
use std::time::Duration;
use ulam_leapers::game::persist::uls::UlsSimulation;
use ulam_leapers::game::simulation::{FinalizedSimulation, FinalizedSimulationToSimulationProgress, Game, PlayerId, Simulation};
use ulam_leapers::util::memory::MemSize;
use crate::gui::widgets::simulation_info::show_finalized_simulation_info_ui;

const MIN_TURNS: u64 = 1_000_000;
const MAX_TURNS: u64 = 1_000_000 * 1_000_000;
const MIN_COMPLETE_SHELLS: u64 = 10;
const MAX_COMPLETE_SHELLS: u64 = 1_000_000;
const MIN_MEMORY_USAGE: MemSize = MemSize::gb(1);
const MAX_MEMORY_USAGE: MemSize = MemSize::tb(4);

enum SimulationResumerWorkerJob {
    Stop,
    ConvertToSimulation(
        FinalizedSimulation,
        Arc<Mutex<FinalizedSimulationToSimulationProgress>>,
        Context,
    ),
}

enum SimulationResumerWorkerResult {
    ConvertedSimulation(Simulation),
}

struct SimulationResumerWorker {
    job_receiver: mpsc::Receiver<SimulationResumerWorkerJob>,
    result_sender: mpsc::Sender<SimulationResumerWorkerResult>,
}

impl SimulationResumerWorker {
    pub fn run(self) {
        loop {
            let job = self.job_receiver.recv().unwrap();
            match job {
                SimulationResumerWorkerJob::Stop => {
                    break;
                }
                SimulationResumerWorkerJob::ConvertToSimulation(
                    finalized_simulation,
                    progress_slot,
                    context,
                ) => {
                    let simulation = finalized_simulation.to_simulation(|progress| {
                        *progress_slot.lock().unwrap() = progress;
                    });
                    self.result_sender
                        .send(SimulationResumerWorkerResult::ConvertedSimulation(
                            simulation,
                        ))
                        .unwrap();
                    context.request_repaint();
                }
            }
        }
    }
}

enum State {
    ResolvingStateChange,
    FinalizedSimulation(FinalizedSimulation),
    Converting(Arc<Mutex<FinalizedSimulationToSimulationProgress>>),
    Simulation(Simulation),
}

pub struct SimulationResumer {
    state: State,

    simulation_limits_input: SimulationLimitsInput,

    submit_to_runner: bool,

    worker: Option<JoinHandle<()>>,
    worker_jobs: mpsc::Sender<SimulationResumerWorkerJob>,
    worker_results: mpsc::Receiver<SimulationResumerWorkerResult>,
}

impl Drop for SimulationResumer {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.worker_jobs
                .send(SimulationResumerWorkerJob::Stop)
                .unwrap();
            if let Err(e) = worker.join() {
                eprintln!("Failed to join worker: {:?}", e);
            }
        }
    }
}

enum FinalizedSimulationUiAction {
    None,
    Submit,
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

        let (job_sender, job_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();

        Ok(Self {
            state: State::FinalizedSimulation(fin_sim),

            simulation_limits_input,

            submit_to_runner: false,

            worker: Some(std::thread::spawn(move || {
                SimulationResumerWorker {
                    job_receiver,
                    result_sender,
                }
                .run()
            })),
            worker_jobs: job_sender,
            worker_results: result_receiver,
        })
    }
}

impl Subwindow for SimulationResumer {
    fn name(&self) -> String {
        "Resumer".to_string()
    }

    fn ui(mut self: Box<Self>, ui: &mut Ui) -> SubwindowResult {
        self.handle_state_changes(ContextOrUi::Ui(ui));

        self.get_subwindow_result()
    }

    fn not_ui(mut self: Box<Self>, ctx: &Context) -> SubwindowResult {
        self.handle_state_changes(ContextOrUi::Context(ctx));

        self.get_subwindow_result()
    }
}

impl SimulationResumer {
    fn get_subwindow_result(mut self: Box<Self>) -> SubwindowResult {
        if self.submit_to_runner {
            if let State::Simulation(sim) =
                std::mem::replace(&mut self.state, State::ResolvingStateChange)
            {
                let limits = self.simulation_limits_input.build_limits();
                let runner = SimulationRunner::new(sim, limits);
                Replace(Box::new(runner))
            } else {
                panic!("Invalid state while trying to submit simulation to runner");
            }
        } else {
            Keep(self)
        }
    }

    fn handle_state_changes(&mut self, mut ctxui: ContextOrUi) {
        if let Ok(result) = self.worker_results.try_recv() {
            match result {
                SimulationResumerWorkerResult::ConvertedSimulation(sim) => {
                    self.state = State::Simulation(sim);
                }
            }
        }

        let old_state = std::mem::replace(&mut self.state, State::ResolvingStateChange);
        self.state = match old_state {
            State::FinalizedSimulation(fin_sim) => {
                if let Some(ui) = ctxui.ui() {
                    match self.show_finalized_simulation_ui(ui, &fin_sim) {
                        FinalizedSimulationUiAction::None => State::FinalizedSimulation(fin_sim),
                        FinalizedSimulationUiAction::Submit => {
                            let progress = Arc::new(Mutex::new(
                                FinalizedSimulationToSimulationProgress::default(),
                            ));
                            self.worker_jobs
                                .send(SimulationResumerWorkerJob::ConvertToSimulation(
                                    fin_sim,
                                    progress.clone(),
                                    ctxui.ctx().clone(),
                                ))
                                .unwrap();
                            State::Converting(progress)
                        }
                    }
                } else {
                    State::FinalizedSimulation(fin_sim)
                }
            }
            State::Converting(progress) => {
                if let Some(ui) = ctxui.ui() {
                    self.show_converting_ui(ui, &progress);
                    // We want regular updates.
                    ui.ctx().request_repaint();
                }
                State::Converting(progress)
            }
            State::Simulation(sim) => {
                self.submit_to_runner = true;
                State::Simulation(sim)
            }
            State::ResolvingStateChange => {
                panic!("Invalid state");
            }
        };
    }

    fn show_converting_ui(
        &mut self,
        ui: &mut Ui,
        progress: &Arc<Mutex<FinalizedSimulationToSimulationProgress>>,
    ) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::Frame::default().show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label("Preparing simulation...");
                    let progress_copy = *progress.lock().unwrap();
                    match progress_copy {
                        FinalizedSimulationToSimulationProgress::UnfreezingChunks(i, total) => {
                            ui.label(format!("Unfreezing chunks: {i} / {total}"));
                        }
                        FinalizedSimulationToSimulationProgress::RecomputingForbiddances(
                            i,
                            total,
                        ) => {
                            ui.label("Finished unfreezing chunks.");
                            ui.label(format!(
                                "Recomputing forbiddances for chunks: {i} / {total}"
                            ));
                        }
                    }
                });
            });
        });
    }

    #[must_use]
    fn show_finalized_simulation_ui(
        &mut self,
        ui: &mut Ui,
        sim: &FinalizedSimulation,
    ) -> FinalizedSimulationUiAction {
        let mut submit = false;

        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::Frame::default().show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    self.show_players_ui(ui, sim);

                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            self.show_info_ui(ui, sim);
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
            FinalizedSimulationUiAction::Submit
        } else {
            FinalizedSimulationUiAction::None
        }
    }

    fn show_players_ui(&mut self, ui: &mut Ui, finalized_simulation: &FinalizedSimulation) {
        let players = finalized_simulation.players();

        ui.horizontal_top(|ui| {
            ui.group(|ui| {
                ScrollArea::new(Vec2b::new(false, true)).show(ui, |ui| {
                    ui.vertical(|ui| {
                        for (i, player) in players.iter().enumerate() {
                            let pid = PlayerId::new((i + 1) as u8);
                            let name = make_player_name(pid);
                            ui.label(format!("{name} attacks"));
                            let mut widget = LeaperAttacksView::new(player.attacks());
                            widget.ui(ui);
                        }
                    });
                });
            });

            let mut relations_widget = PlayerRelationsView::new(players);
            relations_widget.ui(ui);
        });
    }

    fn show_info_ui(&mut self, ui: &mut Ui, finalized_simulation: &FinalizedSimulation) {
        show_finalized_simulation_info_ui(finalized_simulation, ui);
    }
}
