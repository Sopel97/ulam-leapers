use crate::gui::grid_explorer::GridExplorer;
use crate::gui::SubwindowResult::{Keep, Replace};
use crate::gui::{Subwindow, SubwindowResult};
use eframe::egui;
use eframe::egui::{ProgressBar, Ui};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use ulam_leapers::simulation::{
    FinalizedSimulation, Simulation, SimulationError, SimulationLimit, SimulationLimits,
    SimulationProgress,
};
use ulam_leapers::util::time;

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

struct ProgressTracker {
    speed_averaging_window: Duration,
    start_time: std::time::Instant,
    last_progress_update_time: std::time::Instant,
    curr_progress: SimulationProgress,
    average_turns_per_second: f64,
}

impl ProgressTracker {
    pub fn new() -> Self {
        let now = std::time::Instant::now();
        Self {
            speed_averaging_window: Duration::from_secs(3),
            start_time: now,
            last_progress_update_time: now,
            curr_progress: SimulationProgress::default(),
            average_turns_per_second: 0.0,
        }
    }

    pub fn current_progress(&self) -> SimulationProgress {
        self.curr_progress
    }

    pub fn elapsed(&self) -> Duration {
        let now = std::time::Instant::now();
        now.duration_since(self.start_time)
    }

    pub fn eta_to_turns(&self, turns: usize) -> Option<Duration> {
        if turns <= self.curr_progress.turns() {
            return Some(Duration::from_secs(0));
        }

        if self.average_turns_per_second.is_finite() && self.average_turns_per_second > 0.0 {
            let turns_diff = turns - self.curr_progress.turns();
            let eta_secs = turns_diff as f64 / self.average_turns_per_second;
            Some(Duration::from_secs_f64(eta_secs))
        } else {
            None
        }
    }

    pub fn turns_per_second(&self) -> f64 {
        self.average_turns_per_second
    }

    pub fn on_new_progress(&mut self, new_progress: SimulationProgress) {
        if new_progress.turns() < self.curr_progress.turns() {
            panic!("Progress went backwards.");
        }

        let now = std::time::Instant::now();
        let duration_since_last_update = now.duration_since(self.last_progress_update_time);
        // The fraction of the new diff that needs to be replaced in the accumulator.
        let t = (duration_since_last_update.as_secs_f64() / self.speed_averaging_window.as_secs_f64()).min(1.0);
        let turns_diff = new_progress.turns() - self.curr_progress.turns();
        let instant_turns_per_second = turns_diff as f64 / duration_since_last_update.as_secs_f64();
        self.average_turns_per_second = self.average_turns_per_second * (1.0-t) + instant_turns_per_second * t;
        self.last_progress_update_time = now;
        self.curr_progress = new_progress;
    }
}

pub struct SimulationRunner {
    simulation_state: SimulationRunnerState,
    limits: SimulationLimits,
    stop_flag: Arc<AtomicBool>,
    progress_tracker: ProgressTracker,

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
            progress_tracker: ProgressTracker::new(),

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
            self.stop_flag.store(true, Ordering::SeqCst);
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

                let progress = *self.progress.lock().unwrap();
                self.progress_tracker.on_new_progress(progress);
                Self::show_progress(ui, &self.limits, &self.progress_tracker);

                self.simulation_state = match old_simulation_state {
                    SimulationRunnerState::Init(simulation) => {
                        start_simulation(simulation, ui);
                        SimulationRunnerState::Simulating
                    }
                    SimulationRunnerState::Simulating => {
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
    fn show_progress(ui: &mut Ui, limits: &SimulationLimits, progress_tracker: &ProgressTracker) {
        let progress = progress_tracker.current_progress();
        ui.vertical(|ui| {
            if let Some(turns) = limits.turns() {
                let eta = progress_tracker.eta_to_turns(turns);
                let elapsed = progress_tracker.elapsed();
                let turns_per_second_mil = progress_tracker.turns_per_second() / (1024 * 1024) as f64;
                ui.label(format!(
                    "Turns {}M / {}M, {}M per second, Elapsed: {}, ETA: {}",
                    progress.turns() / 1_000_000,
                    turns / 1_000_000,
                    turns_per_second_mil as i64,
                    time::format_duration_hhmmss(elapsed),
                    time::format_opt_duration_hhmmss(eta),
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
