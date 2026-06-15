use crate::gui::grid_render::render::default_player_colors;
use crate::gui::grid_render::samplers::MapLastCollector;
use crate::gui::simulation_runner::SimulationRunner;
use crate::gui::subwindow::SubwindowResult::{Keep, Replace};
use crate::gui::subwindow::{Subwindow, SubwindowResult};
use crate::gui::widgets::simulation_config::{
    SimulationConfigInput, SimulationConfigInputConstraints,
};
use crate::gui::widgets::widget::{JsonWidget, StatefulWidget};
use eframe::egui;
use eframe::egui::{
    pos2, vec2, Button, Color32, ColorImage, Context, FontFamily, FontId, Rect, ScrollArea,
    Slider, Stroke, TextStyle, TextureFilter, TextureOptions, TextureWrapMode, Ui, Vec2,
};
use eframe::epaint::TextureHandle;
use std::ops::RangeInclusive;
use std::sync::mpsc;
use std::thread::JoinHandle;
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::game::sampler::{FrozenGridSampler, SampleCollector};
use ulam_leapers::game::simulation::{Simulation, SimulationLimits};
use ulam_leapers::math::coords::GridPoint;
use ulam_leapers::math::rect::GridRect;
use ulam_leapers::util::memory::MemSize;

const MIN_PLAYER_COUNT: usize = 1;
const DEFAULT_PLAYER_COUNT: usize = 2;
const MAX_PLAYER_COUNT: usize = 8;

const MIN_ATTACK_RADIUS: usize = 3;
const DEFAULT_ATTACK_RADIUS: usize = 5;
const MAX_ATTACK_RADIUS: usize = 7;

const MIN_TURNS: usize = 1_000_000;
const DEFAULT_TURNS: usize = 1_000 * 1_000_000;
const MAX_TURNS: usize = 1_000_000 * 1_000_000;
const MIN_COMPLETE_SHELLS: usize = 10;
const MAX_COMPLETE_SHELLS: usize = 1_000_000;
const MIN_MEMORY_USAGE: MemSize = MemSize::gb(1);
const MAX_MEMORY_USAGE: MemSize = MemSize::tb(4);

const MIN_PREVIEW_SHELLS: usize = 100;
const DEFAULT_PREVIEW_SHELLS: usize = 250;
const MAX_PREVIEW_SHELLS: usize = 1000;

enum SimulationCreatorWorkerJob {
    Stop,
    CancelAll,
    GeneratePreview(Simulation, Context, usize),
}

enum SimulationCreatorWorkerResult {
    PreviewImage(TextureHandle),
}

pub struct SimulationCreator {
    state: SimulationConfigInput,
    last_rendered_state: Option<SimulationConfigInput>,
    state_json_actual: String,
    state_json_ui: String,

    preview_shells: usize,
    last_rendered_preview_shells: usize,

    preview_texture_handle: Option<TextureHandle>,

    worker: Option<JoinHandle<()>>,
    worker_jobs: mpsc::Sender<SimulationCreatorWorkerJob>,
    worker_results: mpsc::Receiver<SimulationCreatorWorkerResult>,
}

impl Default for SimulationCreator {
    fn default() -> Self {
        Self::new()
    }
}

// IMPORTANT: The worker skips jobs and only focuses on the most recent one
struct SimulationCreatorWorker {
    job_receiver: mpsc::Receiver<SimulationCreatorWorkerJob>,
    result_sender: mpsc::Sender<SimulationCreatorWorkerResult>,
}

impl SimulationCreatorWorker {
    pub fn run(self) {
        loop {
            let mut job = self.job_receiver.recv().unwrap();
            if let SimulationCreatorWorkerJob::Stop = job {
                break;
            }

            // Consume jobs as long as there is something newer.
            while let Ok(newer) = self.job_receiver.try_recv() {
                job = newer;

                if let SimulationCreatorWorkerJob::Stop = job {
                    break;
                }
            }

            match job {
                SimulationCreatorWorkerJob::Stop => break,
                SimulationCreatorWorkerJob::CancelAll => {
                    while self.job_receiver.try_recv().is_ok() {}
                }
                SimulationCreatorWorkerJob::GeneratePreview(simulation, ctx, shells) => {
                    self.generate_preview(simulation, ctx, shells);
                }
            }
        }
    }

    fn generate_preview(&self, mut simulation: Simulation, ctx: Context, shells: usize) {
        if shells == 0 {
            return;
        }

        let limits = SimulationLimits::new().with_complete_shell_limit(shells);
        let _ = simulation.simulate(limits);
        let finalized = simulation.finalize();
        let frozen_grid = finalized.grid();

        let colors = default_player_colors();
        let bounds = GridRect::with_size(
            GridPoint::new(-(shells as i32), -(shells as i32)),
            (shells * 2 + 1) as i32,
            (shells * 2 + 1) as i32,
        );

        let collector = MapLastCollector::new(&colors);
        let sampler = FrozenGridSampler::new(&frozen_grid, bounds, colors[0], collector);
        let samples: Array2D<Color32> = sampler.par_sample();
        let image = ColorImage::new(
            [samples.width(), samples.height()],
            samples.as_flat_slice().to_vec(),
        );

        let texture_options = TextureOptions {
            magnification: TextureFilter::Nearest,
            minification: TextureFilter::Linear,
            wrap_mode: TextureWrapMode::ClampToEdge,
            mipmap_mode: None,
        };
        let handle = ctx.load_texture("name", image, texture_options);

        self.result_sender
            .send(SimulationCreatorWorkerResult::PreviewImage(handle))
            .unwrap();

        // The UI should now receive the preview texture, but it's reactive
        // so we should force a repaint.
        ctx.request_repaint();
    }
}

impl SimulationCreator {
    pub fn get_preview_shells_range() -> RangeInclusive<usize> {
        MIN_PREVIEW_SHELLS..=MAX_PREVIEW_SHELLS
    }

    pub fn get_player_count_range() -> RangeInclusive<usize> {
        MIN_PLAYER_COUNT..=MAX_PLAYER_COUNT
    }

    pub fn make_creation_state_constraints() -> SimulationConfigInputConstraints {
        SimulationConfigInputConstraints {
            attack_radius: MIN_ATTACK_RADIUS..=MAX_ATTACK_RADIUS,
            memory_usage: MIN_MEMORY_USAGE..=MAX_MEMORY_USAGE,
            complete_shells: MIN_COMPLETE_SHELLS..=MAX_COMPLETE_SHELLS,
            player_count: Self::get_player_count_range(),
            turns: MIN_TURNS..=MAX_TURNS,
        }
    }

    pub fn new() -> Self {
        let (job_sender, job_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();

        let mut state =
            SimulationConfigInput::new(Self::make_creation_state_constraints()).unwrap();
        state.set_turns_limit(DEFAULT_TURNS).unwrap();
        state.set_player_count(DEFAULT_PLAYER_COUNT).unwrap();
        state.set_attack_radius(DEFAULT_ATTACK_RADIUS).unwrap();

        Self {
            state,
            last_rendered_state: None,
            state_json_actual: String::new(),
            state_json_ui: String::new(),

            preview_shells: DEFAULT_PREVIEW_SHELLS,
            last_rendered_preview_shells: 0,

            preview_texture_handle: None,

            worker: Some(std::thread::spawn(move || {
                SimulationCreatorWorker {
                    job_receiver,
                    result_sender,
                }
                .run()
            })),
            worker_jobs: job_sender,
            worker_results: result_receiver,
        }
    }
}

impl Drop for SimulationCreator {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.worker_jobs
                .send(SimulationCreatorWorkerJob::CancelAll)
                .unwrap();
            self.worker_jobs
                .send(SimulationCreatorWorkerJob::Stop)
                .unwrap();
            if let Err(e) = worker.join() {
                eprintln!("Failed to join worker: {:?}", e);
            }
        }
    }
}

impl Subwindow for SimulationCreator {
    fn name(&self) -> String {
        "Creator".to_owned()
    }

    fn ui(mut self: Box<Self>, ui: &mut Ui) -> SubwindowResult {
        let mut submit = false;

        self.handle_state_json_import_export();

        self.maybe_update_preview(ui.ctx());

        egui::Panel::left("simulation_setup_panel")
            .resizable(false)
            .show_inside(ui, |ui| {
                self.state.ui(ui);
            });

        egui::CentralPanel::no_frame().show_inside(ui, |ui| {
            // Actions
            ui.horizontal(|ui| {
                ui.group(|ui| {
                    ui.scope(|ui| {
                        ui.style_mut()
                            .text_styles
                            .insert(TextStyle::Body, FontId::new(24.0, FontFamily::Proportional));
                        let start_button = Button::new("START")
                            .min_size(vec2(100.0, 40.0))
                            .corner_radius(10.0)
                            .stroke(Stroke::new(4.0, Color32::WHITE));
                        if ui.add(start_button).clicked() {
                            submit = true;
                        }
                    });

                    ui.separator();

                    ui.vertical(|ui| {
                        if ui
                            .button("Export")
                            .on_hover_text("Export to clipboard")
                            .clicked()
                        {
                            ui.copy_text(self.state_json_actual.clone());
                        }

                        if ui
                            .button("Import")
                            .on_hover_text("Import from clipboard")
                            .clicked()
                        {
                            ui.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                        }
                    });

                    ui.input(|i| {
                        for event in &i.events {
                            if let egui::Event::Paste(text) = event {
                                self.state_json_ui = text.clone();
                            }
                        }
                    });

                    Self::show_import_export(ui, &mut self.state_json_ui);
                });
            });

            self.preview_panel(ui);
        });

        if submit {
            let (simulation, limits) = self.state.build_simulation();
            Replace(Box::new(SimulationRunner::new(simulation, limits)))
        } else {
            Keep(self)
        }
    }

    fn not_ui(self: Box<Self>, _ctx: &Context) -> SubwindowResult {
        Keep(self)
    }
}

impl SimulationCreator {
    fn preview_panel(&mut self, ui: &mut Ui) {
        egui::Frame::default().show(ui, |ui| {
            ui.add(
                Slider::new(&mut self.preview_shells, Self::get_preview_shells_range())
                    .integer()
                    .text("Preview shells"),
            );
        });

        egui::Frame::default().show(ui, |ui| {
            let rect = ui.max_rect();

            let mut last_result = None;
            while let Ok(result) = self.worker_results.try_recv() {
                last_result = Some(result);
            }

            match last_result {
                None => { /* No new preview */ }
                Some(SimulationCreatorWorkerResult::PreviewImage(handle)) => {
                    self.preview_texture_handle = Some(handle);
                }
            }

            if let Some(handle) = &self.preview_texture_handle {
                // y-flip via uv
                let painter = ui.painter_at(rect);
                let target_rect_size = rect.width().min(rect.height());
                let target_rect =
                    Rect::from_center_size(rect.center(), Vec2::splat(target_rect_size));
                painter.image(
                    handle.id(),
                    target_rect,
                    Rect::from_min_max(pos2(0.0, 1.0), pos2(1.0, 0.0)),
                    Color32::WHITE,
                );
            }
        });
    }

    fn update_state_json(&mut self) {
        self.state_json_actual = self.state.to_json().to_string();
        let json = &serde_json::from_str(self.state_json_actual.as_str()).unwrap();
        SimulationConfigInput::try_from_json(json, Self::make_creation_state_constraints())
            .unwrap();
        self.state_json_ui = self.state_json_actual.clone();
    }

    fn on_user_changed_state_json(&mut self) {
        let json = &serde_json::from_str(self.state_json_ui.as_str());
        if let Ok(json) = json
            && let Ok(state) =
                SimulationConfigInput::try_from_json(json, Self::make_creation_state_constraints())
        {
            self.state_json_actual = state.to_json().to_string();
            self.state_json_ui = self.state_json_actual.clone();
            self.state = state;
        } else {
            self.state_json_ui = self.state_json_actual.clone();
        }
    }

    fn handle_state_json_import_export(&mut self) {
        if self.state_json_ui == self.state_json_actual {
            // No user interaction, we just update the JSON code.
            self.update_state_json();
        } else {
            // The user has modified the JSON code, we should try parsing it.
            self.on_user_changed_state_json();
        }
    }

    fn needs_preview_update(&self) -> bool {
        match &self.last_rendered_state {
            None => true,
            Some(last_state) => {
                self.last_rendered_preview_shells != self.preview_shells
                    || self.state.requires_preview_update(last_state)
            }
        }
    }

    fn maybe_update_preview(&mut self, ctx: &Context) {
        if self.needs_preview_update() {
            let (simulation, _) = self.state.build_simulation();
            self.worker_jobs
                .send(SimulationCreatorWorkerJob::GeneratePreview(
                    simulation,
                    ctx.clone(),
                    self.preview_shells,
                ))
                .unwrap();

            // Do not forget to set the last rendered state.
            self.last_rendered_state = Some(self.state.clone());
            self.last_rendered_preview_shells = self.preview_shells;
            debug_assert!(!self.needs_preview_update());
        }
    }

    fn show_import_export(ui: &mut Ui, state_json_ui: &mut String) {
        ScrollArea::horizontal().show(ui, |ui| {
            let json_code_block = egui::TextEdit::singleline(state_json_ui)
                .font(egui::TextStyle::Monospace)
                .lock_focus(true)
                .desired_width(f32::INFINITY);
            ui.add(json_code_block);
        });
    }
}
