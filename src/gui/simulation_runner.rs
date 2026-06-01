use crate::gui::{Subwindow, SubwindowResult};
use eframe::egui;
use eframe::egui::Ui;
use std::sync::atomic::AtomicBool;
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

struct SimulationRunner {
    paused_simulation: Option<Simulation>,
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
            paused_simulation: Some(sim),
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

    fn ui(self: Box<Self>, ui: &mut Ui) -> SubwindowResult {}
}
