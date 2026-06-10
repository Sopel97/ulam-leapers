use crate::gui::grid_explorer::GridExplorer;
use crate::gui::subwindow::SubwindowResult::{Keep, Replace};
use crate::gui::subwindow::{Subwindow, SubwindowResult};
use eframe::egui;
use eframe::egui::{Button, Context, ProgressBar, RichText, Ui};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
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
        Context,
    ),
    Finalize(Simulation, Context),
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
            speed_averaging_window: Duration::from_secs(10),
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

    pub fn active_time_elapsed(&self) -> Duration {
        self.last_progress_update_time - self.start_time
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
        // exponential moving average
        // adjust tau early on for faster convergence
        let tau = ((now - self.start_time).as_secs_f64() * 0.2)
            .min(self.speed_averaging_window.as_secs_f64());
        let alpha = 1.0 - (-duration_since_last_update.as_secs_f64() / tau).exp();
        let turns_diff = new_progress.turns() - self.curr_progress.turns();
        let instant_turns_per_second = turns_diff as f64 / duration_since_last_update.as_secs_f64();
        self.average_turns_per_second =
            self.average_turns_per_second * (1.0 - alpha) + instant_turns_per_second * alpha;
        self.last_progress_update_time = now;
        self.curr_progress = new_progress;
    }
}

enum ContextOrUi<'a> {
    Context(&'a Context),
    Ui(&'a mut Ui),
}

impl<'a> ContextOrUi<'a> {
    pub fn ctx(&self) -> &Context {
        match self {
            ContextOrUi::Context(ctx) => ctx,
            ContextOrUi::Ui(ui) => ui.ctx(),
        }
    }

    pub fn ui(&mut self) -> Option<&mut Ui> {
        match self {
            ContextOrUi::Context(_ctx) => None,
            ContextOrUi::Ui(ui) => Some(ui),
        }
    }
}

pub struct SimulationRunner {
    simulation_state: SimulationRunnerState,
    limits: SimulationLimits,
    stop_flag: Arc<AtomicBool>,
    progress_tracker: ProgressTracker,

    submit_to_explorer: bool,
    progress: Arc<Mutex<SimulationProgress>>,
    last_progress: SimulationProgress,
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

            submit_to_explorer: false,
            progress: Default::default(),
            last_progress: Default::default(),
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
        match self.simulation_state {
            SimulationRunnerState::Init(_) => "Simulator".to_string(),
            SimulationRunnerState::Simulating => {
                if let Some(limit_turns) = self.limits.turns() {
                    let progress_turns = self.progress.lock().unwrap().turns();
                    let pct = progress_turns * 100 / limit_turns;
                    format!("Simulator: {}%", pct)
                } else {
                    "Simulator".to_string()
                }
            }
            SimulationRunnerState::Finalizing => "Finalizing...".to_string(),
            SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Paused(_)) => {
                if let Some(limit_turns) = self.limits.turns() {
                    let progress_turns = self.progress.lock().unwrap().turns();
                    let pct = progress_turns * 100 / limit_turns;
                    format!("Paused... {}%", pct)
                } else {
                    "Simulator".to_string()
                }
            }
            SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Finished(_)) => {
                "Finished".to_string()
            }
            SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Finalized(_)) => {
                "Finalized".to_string()
            }
            SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Errored(_, _)) => {
                "Errored".to_string()
            }
            SimulationRunnerState::Closing => "Simulation".to_string(),
        }
    }

    fn ui(mut self: Box<Self>, ui: &mut Ui) -> SubwindowResult {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::Frame::default().show(ui, |ui| {
                self.handle_state_changes(ContextOrUi::Ui(ui));

                let progress = *self.progress.lock().unwrap();
                if progress != self.last_progress {
                    self.progress_tracker.on_new_progress(progress);
                    self.last_progress = progress;
                }
                Self::show_progress(ui, &self.limits, &self.progress_tracker);
            });
        });

        if self.submit_to_explorer {
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
        !matches!(
            self.simulation_state,
            SimulationRunnerState::Simulating | SimulationRunnerState::Finalizing
        )
    }

    fn not_ui(mut self: Box<Self>, ctx: &Context) -> SubwindowResult {
        self.handle_state_changes(ContextOrUi::Context(ctx));

        Keep(self)
    }
}

impl SimulationRunner {
    fn handle_state_changes(&mut self, mut ctxui: ContextOrUi) {
        while let Ok(result) = self.worker_results.try_recv() {
            self.simulation_state = SimulationRunnerState::Idle(result);
        }

        let old_simulation_state = std::mem::replace(
            &mut self.simulation_state,
            SimulationRunnerState::Simulating,
        );

        let start_simulation = |sim, ctx: &Context| {
            self.stop_flag.store(false, Ordering::SeqCst);
            self.worker_jobs
                .send(SimulationRunnerWorkerJob::Simulate(
                    sim,
                    self.limits.clone(),
                    self.progress.clone(),
                    ctx.clone(),
                ))
                .unwrap();
        };

        let finalize_simulation = |sim, ctx: &Context| {
            self.worker_jobs
                .send(SimulationRunnerWorkerJob::Finalize(sim, ctx.clone()))
                .unwrap();
        };

        self.simulation_state = match old_simulation_state {
            SimulationRunnerState::Init(simulation) => {
                start_simulation(simulation, ctxui.ctx());
                SimulationRunnerState::Simulating
            }
            SimulationRunnerState::Simulating => {
                if let Some(ui) = ctxui.ui()
                    && ui
                        .add(Button::new(RichText::new("Pause simulation").heading()))
                        .clicked()
                {
                    self.stop_flag.store(true, Ordering::SeqCst);
                }
                SimulationRunnerState::Simulating
            }
            SimulationRunnerState::Finalizing => {
                if let Some(ui) = ctxui.ui() {
                    ui.label("Finalizing simulation...");
                    ui.spinner();
                }
                SimulationRunnerState::Finalizing
            }
            SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Paused(simulation)) => {
                if let Some(ui) = ctxui.ui() {
                    if ui
                        .add(Button::new(RichText::new("Resume simulation").heading()))
                        .clicked()
                    {
                        start_simulation(simulation, ctxui.ctx());
                        SimulationRunnerState::Simulating
                    } else if ui
                        .add(Button::new(RichText::new("Stop and finalize").heading()))
                        .clicked()
                    {
                        finalize_simulation(simulation, ctxui.ctx());
                        SimulationRunnerState::Finalizing
                    } else {
                        SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Paused(
                            simulation,
                        ))
                    }
                } else {
                    SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Paused(simulation))
                }
            }
            SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Finished(simulation)) => {
                finalize_simulation(simulation, ctxui.ctx());
                SimulationRunnerState::Finalizing
            }
            SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Errored(
                _simulation,
                error,
            )) => {
                panic!("Simulation error {:?}", error);
            }
            SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Finalized(
                finalized_simulation,
            )) => {
                if let Some(ui) = ctxui.ui()
                    && ui
                        .add(Button::new(RichText::new("Explore").heading()))
                        .clicked()
                {
                    self.submit_to_explorer = true;
                }
                SimulationRunnerState::Idle(SimulationRunnerWorkerResult::Finalized(
                    finalized_simulation,
                ))
            }
            SimulationRunnerState::Closing => panic!("Invalid state."),
        };
    }

    fn show_progress(ui: &mut Ui, limits: &SimulationLimits, progress_tracker: &ProgressTracker) {
        let progress = progress_tracker.current_progress();
        ui.vertical(|ui| {
            if let Some(turns) = limits.turns() {
                if progress.turns() >= turns {
                    let elapsed = progress_tracker.active_time_elapsed();
                    let turns_per_second_mil =
                        progress.turns() as f64 / elapsed.as_secs_f64() / (1000.0 * 1000.0);
                    ui.label(format!(
                        "Finished {}M turns in {} at {}M turns per second.",
                        progress.turns() / 1_000_000,
                        time::format_duration_hhmmss(elapsed),
                        turns_per_second_mil as i64,
                    ));
                } else {
                    let eta = progress_tracker.eta_to_turns(turns);
                    let elapsed = progress_tracker.active_time_elapsed();
                    let turns_per_second_mil =
                        progress_tracker.turns_per_second() / (1000.0 * 1000.0);
                    ui.label(format!(
                        "Turns {}M / {}M, {}M per second, Elapsed: {}, ETA: {}",
                        progress.turns() / 1_000_000,
                        turns / 1_000_000,
                        turns_per_second_mil as i64,
                        time::format_duration_hhmmss(elapsed),
                        time::format_opt_duration_hhmmss(eta),
                    ));
                }
                let t = (progress.turns() as f32 / turns as f32).clamp(0.0, 1.0);
                ui.add(ProgressBar::new(t).show_percentage());
            }
            if let Some(memory) = limits.memory() {
                ui.label(format!(
                    "Memory {} / {}",
                    progress.memory_usage().display().si(),
                    memory.display().si()
                ));
                let t = (progress.memory_usage() / memory).clamp(0.0, 1.0);
                ui.add(ProgressBar::new(t as f32).show_percentage());
            }
            if let Some(shells) = limits.complete_shells() {
                let complete_shells = progress.complete_shells();
                let side_cells = complete_shells.max(1) * 2 - 1;
                let cells = side_cells * side_cells;
                ui.label(format!(
                    "Complete shells {} / {} ({}M cells)",
                    complete_shells,
                    shells,
                    cells / (1000 * 1000),
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
