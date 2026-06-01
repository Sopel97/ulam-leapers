use crate::gui::SubwindowResult::Keep;
use crate::gui::{Subwindow, SubwindowResult};
use eframe::egui;
use eframe::egui::{ProgressBar, Slider, Ui};
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
}

enum SimulationRunnerWorkerResult {
    Errored(Simulation, SimulationError),
    Paused(Simulation),
    Finished(Simulation),
    Finalized(FinalizedSimulation),
}

enum SimulationRunnerState {
    Idle(SimulationRunnerWorkerResult),
    Simulating,
    Finalizing,
}

struct SimulationRunner {
    simulation_state: SimulationRunnerState,
    limits: SimulationLimits,
    stop_flag: Arc<AtomicBool>,

    progress: Arc<Mutex<SimulationProgress>>,
    worker: Option<JoinHandle<()>>,
    worker_jobs: mpsc::Sender<SimulationRunnerWorkerJob>,
    worker_results: mpsc::Receiver<SimulationRunnerWorkerResult>,
}

impl SimulationRunner {
    fn new(sim: Simulation, mut limits: SimulationLimits) -> Self {
        let (job_sender, job_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();

        let stop_flag = Arc::new(AtomicBool::new(false));
        limits = limits.with_stop_flag_limit(stop_flag.clone());

        Self {
            simulation_state: SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Paused(
                sim,
            )),
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
                    };
                }
            })),
            worker_jobs: job_sender,
            worker_results: result_receiver,
        }
    }
}

impl Subwindow for SimulationRunner {
    fn name(&self) -> String {
        "SimulationRunner".to_string()
    }

    fn is_closeable(&self) -> bool {
        false
    }

    fn ui(mut self: Box<Self>, ui: &mut Ui) -> SubwindowResult {
        egui::CentralPanel::no_frame().show_inside(ui, |ui| {
            egui::Frame::default().show(ui, |ui| {
                while let Ok(result) = self.worker_results.try_recv() {
                    self.simulation_state = SimulationRunnerState::Idle(result);
                }

                let old_simulation_state = std::mem::replace(
                    &mut self.simulation_state,
                    SimulationRunnerState::Simulating,
                );
                self.simulation_state = match old_simulation_state {
                    SimulationRunnerState::Simulating => {
                        // TODO: progress bars
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
                    SimulationRunnerState::Idle(state) => {
                        match state {
                            SimulationRunnerWorkerResult::Paused(simulation) => {
                                if ui.button("Resume simulation").clicked() {
                                    self.stop_flag.store(false, Ordering::SeqCst);
                                    self.worker_jobs
                                        .send(SimulationRunnerWorkerJob::Simulate(
                                            simulation,
                                            self.limits.clone(),
                                            self.progress.clone(),
                                            ui.ctx().clone(),
                                        ))
                                        .unwrap();
                                    SimulationRunnerState::Simulating
                                } else {
                                    SimulationRunnerState::Idle(
                                        SimulationRunnerWorkerResult::Paused(simulation),
                                    )
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
                                SimulationRunnerState::Idle(
                                    SimulationRunnerWorkerResult::Finalized(finalized_simulation),
                                )
                                // TODO: And whatever UI we need
                            }
                        }
                    }
                }
            });
        });

        Keep(self)
    }
}
