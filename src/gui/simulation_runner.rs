use crate::gui::SubwindowResult::{Keep, Replace};
use crate::gui::grid_explorer::GridExplorer;
use crate::gui::{Subwindow, SubwindowResult};
use eframe::egui;
use eframe::egui::{ProgressBar, Ui};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use ulam_leapers::simulation::{
    FinalizedSimulation, Simulation, SimulationError, SimulationLimit, SimulationLimits,
    SimulationProgress,
};

enum SimulationRunnerWorkerJob {
    Simulate(
        Simulation,
        SimulationLimits,
        Arc<Mutex<SimulationProgress>>,
        egui::Context,
    ),
    Finalize(Simulation, egui::Context),
    Stop,
}

enum SimulationRunnerWorkerResult {
    Errored(Simulation, SimulationError),
    Paused(Simulation),
    Finished(Simulation),
    Finalized(FinalizedSimulation),
}

enum SimulationRunnerState {
    Idle(SimulationRunnerWorkerResult),
    Init(Simulation),
    Simulating,
    Finalizing,
    Closing,
}

pub struct SimulationRunner {
    simulation_state: SimulationRunnerState,
    limits: SimulationLimits,
    stop_flag: Arc<AtomicBool>,

    progress: Arc<Mutex<SimulationProgress>>,
    worker: Option<JoinHandle<()>>,
    worker_jobs: mpsc::Sender<SimulationRunnerWorkerJob>,
    worker_results: mpsc::Receiver<SimulationRunnerWorkerResult>,
}

impl SimulationRunner {
    pub fn new(sim: Simulation, mut limits: SimulationLimits) -> Self {
        let (job_sender, job_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();

        let stop_flag = Arc::new(AtomicBool::new(false));
        limits = limits.with_stop_flag_limit(stop_flag.clone());

        Self {
            simulation_state: SimulationRunnerState::Init(sim),
            limits,
            stop_flag,

            progress: Default::default(),
            worker: Some(std::thread::spawn(move || {
                let job_receiver = job_receiver;
                let result_sender = result_sender;
                loop {
                    let job = job_receiver.recv().unwrap();

                    match job {
                        SimulationRunnerWorkerJob::Simulate(
                            mut simulation,
                            limits,
                            progress,
                            ctx,
                        ) => {
                            let progress_callback = |p| {
                                *progress.lock().unwrap() = p;
                                ctx.request_repaint();
                            };
                            let result =
                                simulation.simulate_with_callback(limits, progress_callback);
                            match result {
                                Ok(hit_limit) => match hit_limit {
                                    SimulationLimit::StopFlag => {
                                        result_sender
                                            .send(SimulationRunnerWorkerResult::Paused(simulation))
                                            .unwrap();
                                    }
                                    _ => {
                                        result_sender
                                            .send(SimulationRunnerWorkerResult::Finished(
                                                simulation,
                                            ))
                                            .unwrap();
                                    }
                                },
                                Err(error) => {
                                    result_sender
                                        .send(SimulationRunnerWorkerResult::Errored(
                                            simulation, error,
                                        ))
                                        .unwrap();
                                }
                            };
                            ctx.request_repaint();
                        }
                        SimulationRunnerWorkerJob::Finalize(simulation, ctx) => {
                            result_sender
                                .send(SimulationRunnerWorkerResult::Finalized(
                                    simulation.finalize(),
                                ))
                                .unwrap();
                            ctx.request_repaint();
                        }
                        SimulationRunnerWorkerJob::Stop => break,
                    };
                }
            })),
            worker_jobs: job_sender,
            worker_results: result_receiver,
        }
    }
}

impl Drop for SimulationRunner {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.worker_jobs
                .send(SimulationRunnerWorkerJob::Stop)
                .unwrap();
            if let Err(e) = worker.join() {
                eprintln!("Failed to join worker: {:?}", e);
            }
        }
    }
}

impl Subwindow for SimulationRunner {
    fn name(&self) -> String {
        "SimulationRunner".to_string()
    }

    fn ui(mut self: Box<Self>, ui: &mut Ui) -> SubwindowResult {
        let mut submit_to_explorer = false;

        egui::CentralPanel::no_frame().show_inside(ui, |ui| {
            egui::Frame::default().show(ui, |ui| {
                while let Ok(result) = self.worker_results.try_recv() {
                    self.simulation_state = SimulationRunnerState::Idle(result);
                }

                let old_simulation_state = std::mem::replace(
                    &mut self.simulation_state,
                    SimulationRunnerState::Simulating,
                );

                let start_simulation = |sim, ui: &mut Ui| {
                    self.stop_flag.store(false, Ordering::SeqCst);
                    self.worker_jobs
                        .send(SimulationRunnerWorkerJob::Simulate(
                            sim,
                            self.limits.clone(),
                            self.progress.clone(),
                            ui.ctx().clone(),
                        ))
                        .unwrap();
                };

                self.simulation_state = match old_simulation_state {
                    SimulationRunnerState::Init(simulation) => {
                        start_simulation(simulation, ui);
                        SimulationRunnerState::Simulating
                    }
                    SimulationRunnerState::Simulating => {
                        let progress = *self.progress.lock().unwrap();
                        Self::show_progress(ui, &self.limits, progress);
                        if ui.button("Pause simulation").clicked() {
                            self.stop_flag.store(true, Ordering::SeqCst);
                        }
                        SimulationRunnerState::Simulating
                    }
                    SimulationRunnerState::Finalizing => {
                        ui.label("Finalizing simulation...");
                        ui.spinner();
                        SimulationRunnerState::Finalizing
                    }
                    SimulationRunnerState::Idle(state) => match state {
                        SimulationRunnerWorkerResult::Paused(simulation) => {
                            if ui.button("Resume simulation").clicked() {
                                start_simulation(simulation, ui);
                                SimulationRunnerState::Simulating
                            } else {
                                SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Paused(
                                    simulation,
                                ))
                            }
                        }
                        SimulationRunnerWorkerResult::Finished(simulation) => {
                            self.worker_jobs
                                .send(SimulationRunnerWorkerJob::Finalize(
                                    simulation,
                                    ui.ctx().clone(),
                                ))
                                .unwrap();
                            SimulationRunnerState::Finalizing
                        }
                        SimulationRunnerWorkerResult::Errored(_simulation, error) => {
                            panic!("Simulation error {:?}", error);
                        }
                        SimulationRunnerWorkerResult::Finalized(finalized_simulation) => {
                            if ui.button("Explore").clicked() {
                                submit_to_explorer = true;
                            }
                            SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Finalized(
                                finalized_simulation,
                            ))
                        }
                    },
                    SimulationRunnerState::Closing => panic!("Invalid state.")
                }
            });
        });

        if submit_to_explorer {
            if let SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Finalized(
                finalized_simulation,
            )) = std::mem::replace(&mut self.simulation_state, SimulationRunnerState::Closing)
            {
                Replace(Box::new(GridExplorer::new(finalized_simulation)))
            } else {
                panic!("Tried submitting to explorer while in an unsuitable state.")
            }
        } else {
            Keep(self)
        }
    }

    fn is_closeable(&self) -> bool {
        false
    }
}

impl SimulationRunner {
    fn show_progress(ui: &mut Ui, limits: &SimulationLimits, progress: SimulationProgress) {
        ui.vertical(|ui| {
            if let Some(turns) = limits.turns() {
                ui.label(format!(
                    "Turns {}M / {}M",
                    progress.turns() / 1_000_000,
                    turns / 1_000_000
                ));
                let t = (progress.turns() as f32 / turns as f32).clamp(0.0, 1.0);
                ui.add(ProgressBar::new(t).show_percentage());
            }
            if let Some(memory) = limits.memory() {
                const MEBIBYTE: usize = 1024 * 1024;
                ui.label(format!(
                    "Memory {}MiB / {}MiB",
                    progress.memory_usage() / MEBIBYTE,
                    memory / MEBIBYTE
                ));
                let t = (progress.memory_usage() as f32 / memory as f32).clamp(0.0, 1.0);
                ui.add(ProgressBar::new(t).show_percentage());
            }
            if let Some(shells) = limits.complete_shells() {
                ui.label(format!(
                    "Complete shells {} / {}",
                    progress.complete_shells(),
                    shells
                ));
                let t = (progress.complete_shells() as f32 / shells as f32).clamp(0.0, 1.0);
                ui.add(
                    ProgressBar::new(t)
                        .text("Complete shells")
                        .show_percentage(),
                );
            }
        });
    }
}
