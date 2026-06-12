use crate::gui::grid_render::default_player_colors;
use crate::gui::simulation_runner::SimulationRunner;
use crate::gui::subwindow::SubwindowResult::{Keep, Replace};
use crate::gui::subwindow::{Subwindow, SubwindowResult};
use crate::gui::widgets::leaper_attacks::LeaperAttacksInput;
use eframe::egui;
use eframe::egui::{
    Checkbox, Color32, ColorImage, Context, Rect, ScrollArea, Slider, TextureFilter,
    TextureOptions, TextureWrapMode, Ui, Vec2, Vec2b, pos2,
};
use eframe::epaint::TextureHandle;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::mpsc;
use std::thread::JoinHandle;
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::game::piece::LeaperAttacks;
use ulam_leapers::game::sampler::{FrozenGridSampler, SampleCollector};
use ulam_leapers::game::simulation::{PlayerId, Simulation, SimulationLimits};
use ulam_leapers::math::coords::{GridPoint, GridVector};
use ulam_leapers::math::rect::GridRect;
use ulam_leapers::util::memory::MemSize;
use crate::gui::widgets::player_relations::PlayerRelationsInput;

const MIN_PLAYER_COUNT: usize = 1;
const DEFAULT_PLAYER_COUNT: usize = 2;
const MAX_PLAYER_COUNT: usize = 8;
const MAX_PIECE_RANGE: usize = 5;

const MIN_TURNS_M: usize = 1;
const DEFAULT_TURNS_M: usize = 1_000;
const MAX_TURNS_M: usize = 1_000_000;
const MIN_COMPLETE_SHELLS: usize = 10;
const MAX_COMPLETE_SHELLS: usize = 1_000_000;
const MIN_MEMORY_USAGE: MemSize = MemSize::gb(1);
const MAX_MEMORY_USAGE: MemSize = MemSize::tb(4);

const MIN_PREVIEW_SHELLS: usize = 100;
const DEFAULT_PREVIEW_SHELLS: usize = 250;
const MAX_PREVIEW_SHELLS: usize = 1000;

#[derive(Debug, Eq, PartialEq, Clone)]
struct LimitsState {
    memory_usage: usize,
    turns_m: usize,
    complete_shells: usize,
}

impl LimitsState {
    fn try_from_json(json: &Value) -> Option<LimitsState> {
        let memory_usage = json["memory_usage"].as_u64()? as usize;
        let turns = json["turns"].as_u64()? as usize;
        let slf = LimitsState {
            memory_usage: memory_usage.max(MIN_MEMORY_USAGE.bytes()),
            turns_m: (turns / 1000 / 1000).max(1),
            complete_shells: json["complete_shells"].as_u64()? as usize,
        };

        if slf.memory_usage < MIN_MEMORY_USAGE.bytes()
            || slf.memory_usage > MAX_MEMORY_USAGE.bytes()
            || slf.turns_m < MIN_TURNS_M
            || slf.turns_m > MAX_TURNS_M
        {
            return None;
        }

        Some(slf)
    }
}

impl LimitsState {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "memory_usage": self.memory_usage,
            "turns": self.turns_m * 1000 * 1000,
            "complete_shells": self.complete_shells,
        })
    }
}

impl Default for LimitsState {
    fn default() -> Self {
        Self {
            memory_usage: MAX_MEMORY_USAGE.bytes(),
            turns_m: DEFAULT_TURNS_M,
            complete_shells: MAX_COMPLETE_SHELLS,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
struct CreationState {
    player_count: usize,
    player_configs: Vec<LeaperAttacksInput>,
    player_relations: PlayerRelationsInput,
    limits: LimitsState,
    preview_shells: usize,
}

impl Default for CreationState {
    fn default() -> Self {
        let mut player_configs = Vec::with_capacity(MAX_PLAYER_COUNT);
        player_configs.resize_with(DEFAULT_PLAYER_COUNT, || {
            LeaperAttacksInput::new(MAX_PIECE_RANGE)
        });
        
        let player_relations = PlayerRelationsInput::new(DEFAULT_PLAYER_COUNT);
        
        CreationState {
            player_count: DEFAULT_PLAYER_COUNT,
            player_configs,
            player_relations,
            limits: Default::default(),
            preview_shells: DEFAULT_PREVIEW_SHELLS,
        }
    }
}

impl CreationState {
    fn to_json(&self) -> Value {
        json!({
            "player_count": self.player_count,
            "player_configs": self.player_configs.iter().take(self.player_count).map(|p| p.to_json()).collect::<Vec<_>>(),
            "player_relations": self.player_relations.to_json(self.player_count),
            "limits": self.limits.to_json(),
            "preview_shells": self.preview_shells,
        })
    }

    fn try_from_json(json: &Value) -> Option<CreationState> {
        let player_configs_array = json["player_configs"].as_array()?;
        let mut slf = CreationState {
            player_count: json["player_count"].as_u64()? as usize,
            player_configs: player_configs_array
                .iter()
                .map(LeaperAttacksInput::try_from_json)
                .collect::<Option<Vec<_>>>()?,
            player_relations: PlayerRelationsInput::try_from_json(&json["player_relations"])?,
            limits: LimitsState::try_from_json(&json["limits"])?,
            preview_shells: json["preview_shells"].as_u64()? as usize,
        };

        if slf.player_count > MAX_PLAYER_COUNT
            || slf.player_configs.len() != slf.player_count
            || slf.preview_shells > MAX_PREVIEW_SHELLS
            || slf.preview_shells < MIN_PREVIEW_SHELLS
        {
            return None;
        }

        Some(slf)
    }
}

enum SimulationCreatorWorkerJob {
    Stop,
    CancelAll,
    GeneratePreview(Simulation, Context, usize),
}

enum SimulationCreatorWorkerResult {
    PreviewImage(TextureHandle),
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
enum SimulationCreatorAction {
    Submit,
}

pub struct SimulationCreator {
    state: CreationState,
    last_rendered_state: Option<CreationState>,
    state_json_actual: String,
    state_json_ui: String,

    // TODO: maybe integrate with GridRender, though the fact that we are splitting
    //       the work into a worker thread complicates this. May not be worth it unless
    //       GridRender gains more functionality.
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

#[derive(Debug)]
struct LastColorCollector<'a> {
    colors: &'a [Color32],
}

impl<'a> SampleCollector for LastColorCollector<'a> {
    type InputType = PlayerId;
    type AccumulatorType = Color32;
    type OutputType = Color32;

    fn zero(&self) -> Self::AccumulatorType {
        Color32::from_rgb(0, 0, 0)
    }

    fn push(&self, acc: &mut Self::AccumulatorType, input: Self::InputType) {
        *acc = self.colors[input.index()]
    }

    fn finalize(&self, acc: Self::AccumulatorType, _size: (usize, usize)) -> Self::OutputType {
        acc
    }
}

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

        let collector = LastColorCollector {
            colors: colors.as_slice(),
        };
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
    pub fn new() -> Self {
        let (job_sender, job_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();

        Self {
            state: CreationState::default(),
            last_rendered_state: None,
            state_json_actual: String::new(),
            state_json_ui: String::new(),

            preview_texture_handle: None,

            // IMPORTANT: The worker skips jobs and only focuses on the most recent one
            // TODO: clearer abstraction for this.
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

    pub fn to_simulation(&self) -> (Simulation, SimulationLimits) {
        let mut sim = Simulation::new();

        for player_config in self.state.player_configs.iter() {
            sim.add_player(player_config.build_leaper_attacks());
        }

        let enemy_map = self.state.player_relations.build_attacker_attacked_pairs();
        for (attacker, attacked) in enemy_map {
            sim.add_player_enemy(attacker, attacked);
        }

        let limits = SimulationLimits::new()
            .with_memory_limit(MemSize::b(self.state.limits.memory_usage))
            .with_turn_limit(self.state.limits.turns_m * 1_000_000)
            .with_complete_shell_limit(self.state.limits.complete_shells);

        (sim, limits)
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
                if let Some(SimulationCreatorAction::Submit) = self.simulation_setup_panel(ui) {
                    submit = true
                };
            });

        egui::CentralPanel::no_frame().show_inside(ui, |ui| {
            self.preview_panel(ui);
        });

        if submit {
            let (simulation, limits) = self.to_simulation();
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
    #[must_use]
    fn simulation_setup_panel(&mut self, ui: &mut Ui) -> Option<SimulationCreatorAction> {
        ui.vertical(|ui| {
            if ui
                .add(
                    Slider::new(
                        &mut self.state.player_count,
                        MIN_PLAYER_COUNT..=MAX_PLAYER_COUNT,
                    )
                    .integer()
                    .text("Player count"),
                )
                .changed()
            {
                self.state
                    .player_configs
                    .resize_with(self.state.player_count, || {
                        LeaperAttacksInput::new(MAX_PIECE_RANGE)
                    });
                
                self.state.player_relations.set_player_count(self.state.player_count);
            }

            self.show_all_configs(ui)
        })
        .inner
    }

    fn preview_panel(&mut self, ui: &mut Ui) {
        egui::Frame::default().show(ui, |ui| {
            ui.add(
                Slider::new(
                    &mut self.state.preview_shells,
                    MIN_PREVIEW_SHELLS..=MAX_PREVIEW_SHELLS,
                )
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
        CreationState::try_from_json(json).unwrap();
        self.state_json_ui = self.state_json_actual.clone();
    }

    fn on_user_changed_state_json(&mut self) {
        let json = &serde_json::from_str(self.state_json_ui.as_str());
        if let Ok(json) = json
            && let Some(state) = CreationState::try_from_json(json)
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

    fn maybe_update_preview(&mut self, ctx: &Context) {
        let needs_update = match &self.last_rendered_state {
            None => true,
            Some(last_state) => {
                // Ignore limit config because it doesn't affect the preview.
                last_state.player_count != self.state.player_count
                    || last_state.player_configs != self.state.player_configs
                    || last_state.player_relations != self.state.player_relations
                    || last_state.preview_shells != self.state.preview_shells
            }
        };

        if needs_update {
            let (simulation, _) = self.to_simulation();
            self.worker_jobs
                .send(SimulationCreatorWorkerJob::GeneratePreview(
                    simulation,
                    ctx.clone(),
                    self.state.preview_shells,
                ))
                .unwrap();
            self.last_rendered_state = Some(self.state.clone());
        }
    }

    fn show_limits(ui: &mut Ui, limits: &mut LimitsState) {
        ui.group(|ui| {
            ui.label("Limits:");
            ui.label("Turns:");
            ui.add(
                Slider::new(&mut limits.turns_m, MIN_TURNS_M..=MAX_TURNS_M)
                    .integer()
                    .logarithmic(true)
                    .suffix(" mil"),
            );

            ui.label("Complete shells:");
            ui.add(
                Slider::new(
                    &mut limits.complete_shells,
                    MIN_COMPLETE_SHELLS..=MAX_COMPLETE_SHELLS,
                )
                .integer()
                .logarithmic(true),
            );

            ui.label("Memory usage:");
            ui.add(
                Slider::new(
                    &mut limits.memory_usage,
                    MIN_MEMORY_USAGE.bytes()..=MAX_MEMORY_USAGE.bytes(),
                )
                .integer()
                .logarithmic(true)
                .custom_formatter(|s, _| MemSize::b(s as usize).display().si().to_string()),
            );
        });
    }

    fn show_import_export(ui: &mut Ui, state_json_ui: &mut String) {
        ScrollArea::both().show(ui, |ui| {
            let line_count = state_json_ui.lines().count();
            let json_code_block = egui::TextEdit::multiline(state_json_ui)
                .font(egui::TextStyle::Monospace)
                .desired_rows(line_count + 1)
                .lock_focus(true)
                .desired_width(f32::INFINITY);
            ui.add(json_code_block);
        });
    }

    fn show_player_configs(&mut self, ui: &mut Ui) {
        egui::Frame::default().show(ui, |ui| {
            ui.vertical(|ui| {
                // Players
                for (i, player_config) in self.state.player_configs.iter_mut().enumerate() {
                    ui.group(|ui| {
                        Self::show_player_config(ui, player_config, i + 1);
                    });
                }
            });
        });
    }

    #[must_use]
    fn show_all_configs(&mut self, ui: &mut Ui) -> Option<SimulationCreatorAction> {
        let mut submit = false;

        ui.horizontal_top(|ui| {
            ScrollArea::new(Vec2b::new(false, true))
                .max_width(300.0)
                .show(ui, |ui| {
                    self.show_player_configs(ui);
                });

            egui::Frame::default().show(ui, |ui| {
                ui.vertical(|ui| {
                    self.state.player_relations.ui(ui);

                    Self::show_limits(ui, &mut self.state.limits);

                    // Actions
                    ui.group(|ui| {
                        if ui.button("Start").clicked() {
                            submit = true;
                        }
                    });

                    Self::show_import_export(ui, &mut self.state_json_ui);
                });
            });
        });

        if submit {
            Some(SimulationCreatorAction::Submit)
        } else {
            None
        }
    }

    fn show_player_config(ui: &mut Ui, player_config: &mut LeaperAttacksInput, pid: usize) {
        ui.horizontal(|ui| {
            egui::Frame::default().show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(format!("P{}", pid));
                });
            });

            player_config.ui(ui);

            // Some space.
            egui::Frame::default().show(ui, |_ui| {});
        });
    }
}
