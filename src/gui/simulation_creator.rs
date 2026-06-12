use crate::gui::grid_render::default_player_colors;
use crate::gui::simulation_runner::SimulationRunner;
use crate::gui::subwindow::SubwindowResult::{Keep, Replace};
use crate::gui::subwindow::{Subwindow, SubwindowResult};
use crate::gui::widgets::leaper_attacks::{LeaperAttacksInput, LeaperAttacksInputConstraints};
use crate::gui::widgets::player_relations::{
    PlayerRelationsInput, PlayerRelationsInputConstraints,
};
use crate::gui::widgets::simulation_limits::{SimulationLimitsConstraints, SimulationLimitsInput};
use crate::gui::widgets::widget::{JsonWidget, JsonWidgetError, StatefulWidget, WidgetError};
use eframe::egui;
use eframe::egui::{
    pos2, Color32, ColorImage, Context, Rect, ScrollArea, Slider,
    TextureFilter, TextureOptions, TextureWrapMode, Ui, Vec2, Vec2b,
};
use eframe::epaint::TextureHandle;
use serde_json::{json, Value};
use std::ops::RangeInclusive;
use std::sync::mpsc;
use std::thread::JoinHandle;
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::game::sampler::{FrozenGridSampler, SampleCollector};
use ulam_leapers::game::simulation::{PlayerId, Simulation, SimulationLimits};
use ulam_leapers::math::coords::GridPoint;
use ulam_leapers::math::rect::GridRect;
use ulam_leapers::util::json::SerdeJsonValueExt;
use ulam_leapers::util::memory::MemSize;

const MIN_PLAYER_COUNT: usize = 1;
const DEFAULT_PLAYER_COUNT: usize = 2;
const MAX_PLAYER_COUNT: usize = 8;
const MAX_PIECE_RANGE: usize = 5;

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

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct CreationStateConstraints {
    pub attack_radius: RangeInclusive<usize>,
    pub player_count: RangeInclusive<usize>,
    pub memory_usage: RangeInclusive<MemSize>,
    pub turns: RangeInclusive<usize>,
    pub complete_shells: RangeInclusive<usize>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
struct CreationState {
    player_count: usize,
    player_configs: Vec<LeaperAttacksInput>,
    player_relations: PlayerRelationsInput,
    simulation_limits: SimulationLimitsInput,

    constraints: CreationStateConstraints,
}

impl CreationState {
    fn new(constraints: CreationStateConstraints) -> Result<Self, WidgetError> {
        let player_count = *constraints.player_count.start();
        let mut player_configs = vec![];
        player_configs.resize_with(player_count, || {
            LeaperAttacksInput::new(constraints.leaper_attacks_input_constraints())
        });

        let player_relations =
            PlayerRelationsInput::new(constraints.player_relations_input_constraints());

        let simulation_limits =
            SimulationLimitsInput::new(constraints.simulation_limits_input_constraints());

        Ok(CreationState {
            player_count,
            player_configs,
            player_relations,
            simulation_limits,
            constraints,
        })
    }

    pub fn set_turns_limit(&mut self, turns: usize) -> Result<(), WidgetError> {
        self.simulation_limits.set_turns(turns)
    }

    pub fn set_player_count(&mut self, player_count: usize) -> Result<(), WidgetError> {
        self.set_player_count_ignore_current(player_count)
    }

    fn on_player_count_changed(&mut self) -> Result<(), WidgetError> {
        self.set_player_count_ignore_current(self.player_count)
    }

    /// Ignores the current value of `self.player_count`.
    fn set_player_count_ignore_current(&mut self, player_count: usize) -> Result<(), WidgetError> {
        if !self.constraints.player_count.contains(&player_count) {
            return Err(WidgetError::ConstraintViolation(format!(
                "Player count {} outside of allowed range {:?}",
                player_count, self.constraints.player_count
            )));
        }

        self.player_configs.resize_with(player_count, || {
            LeaperAttacksInput::new(self.constraints.leaper_attacks_input_constraints())
        });
        self.player_relations.set_player_count(player_count)?;
        self.player_count = player_count;

        Ok(())
    }
}

impl CreationStateConstraints {
    pub fn leaper_attacks_input_constraints(&self) -> LeaperAttacksInputConstraints {
        LeaperAttacksInputConstraints {
            radius: self.attack_radius.clone(),
        }
    }

    pub fn player_relations_input_constraints(&self) -> PlayerRelationsInputConstraints {
        PlayerRelationsInputConstraints {
            player_count: self.player_count.clone(),
        }
    }

    pub fn simulation_limits_input_constraints(&self) -> SimulationLimitsConstraints {
        SimulationLimitsConstraints {
            memory_usage: self.memory_usage.clone(),
            turns: self.turns.clone(),
            complete_shells: self.complete_shells.clone(),
        }
    }
}

impl JsonWidget for CreationState {
    type ConstraintsType = CreationStateConstraints;

    fn to_json(&self) -> Value {
        json!({
            "player_count": self.player_count,
            "player_configs": self.player_configs.iter().take(self.player_count).map(|p| p.to_json()).collect::<Vec<_>>(),
            "player_relations": self.player_relations.to_json(),
            "simulation_limits": self.simulation_limits.to_json(),
        })
    }

    fn try_from_json(
        json: &Value,
        constraints: Self::ConstraintsType,
    ) -> Result<Self, JsonWidgetError> {
        let leaper_attacks_constraints = constraints.leaper_attacks_input_constraints();
        let player_relations_constraints = constraints.player_relations_input_constraints();
        let simulation_limits_constraints = constraints.simulation_limits_input_constraints();

        let player_count = json.read_u64("player_count")? as usize;
        if player_count > MAX_PLAYER_COUNT {
            return Err(WidgetError::ConstraintViolation(format!(
                "Player count {} is outside of allowed range {:?}",
                player_count, constraints.player_count
            ))
            .into());
        }

        let player_configs = json
            .read_array("player_configs")?
            .iter()
            .map(|v| LeaperAttacksInput::try_from_json(v, leaper_attacks_constraints.clone()))
            .collect::<Result<Vec<_>, _>>()?;

        if player_configs.len() != player_count {
            return Err(WidgetError::InvalidState(format!(
                "Number of player configs {} does not match the number of players {}",
                player_configs.len(),
                player_count
            ))
            .into());
        }

        let player_relations = PlayerRelationsInput::try_from_json(
            json.read_value("player_relations")?,
            player_relations_constraints,
        )?;

        if player_relations.player_count() != player_count {
            return Err(WidgetError::InvalidState(format!(
                "Player relations map player count {} does not match the number of players {}",
                player_relations.player_count(),
                player_count
            ))
            .into());
        }

        let simulation_limits = SimulationLimitsInput::try_from_json(
            json.read_value("simulation_limits")?,
            simulation_limits_constraints,
        )?;

        Ok(Self {
            player_count,
            player_configs,
            player_relations,
            simulation_limits,
            constraints,
        })
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

    preview_shells: usize,
    last_rendered_preview_shells: usize,

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
    pub fn get_preview_shells_range() -> RangeInclusive<usize> {
        MIN_PREVIEW_SHELLS..=MAX_PREVIEW_SHELLS
    }

    pub fn get_player_count_range() -> RangeInclusive<usize> {
        MIN_PLAYER_COUNT..=MAX_PLAYER_COUNT
    }

    pub fn make_creation_state_constraints() -> CreationStateConstraints {
        CreationStateConstraints {
            attack_radius: MAX_PIECE_RANGE..=MAX_PIECE_RANGE,
            memory_usage: MIN_MEMORY_USAGE..=MAX_MEMORY_USAGE,
            complete_shells: MIN_COMPLETE_SHELLS..=MAX_COMPLETE_SHELLS,
            player_count: Self::get_player_count_range(),
            turns: MIN_TURNS..=MAX_TURNS,
        }
    }

    pub fn new() -> Self {
        let (job_sender, job_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();

        let mut state = CreationState::new(Self::make_creation_state_constraints()).unwrap();
        state.set_turns_limit(DEFAULT_TURNS).unwrap();
        state.set_player_count(DEFAULT_PLAYER_COUNT).unwrap();

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

    pub fn to_simulation(&self) -> (Simulation, SimulationLimits) {
        let mut sim = Simulation::new();

        for player_config in self.state.player_configs.iter() {
            sim.add_player(player_config.build_leaper_attacks());
        }

        let enemy_map = self.state.player_relations.build_attacker_attacked_pairs();
        for (attacker, attacked) in enemy_map {
            sim.add_player_enemy(attacker, attacked);
        }

        let limits = self.state.simulation_limits.build_limits();

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
                    Slider::new(&mut self.state.player_count, Self::get_player_count_range())
                        .integer()
                        .text("Player count"),
                )
                .changed()
            {
                self.state
                    .on_player_count_changed()
                    .expect("The slider should be within the allowed range");
            }

            self.show_all_configs(ui)
        })
        .inner
    }

    fn preview_panel(&mut self, ui: &mut Ui) {
        egui::Frame::default().show(ui, |ui| {
            ui.add(
                Slider::new(
                    &mut self.preview_shells,
                    Self::get_preview_shells_range(),
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
        CreationState::try_from_json(json, Self::make_creation_state_constraints()).unwrap();
        self.state_json_ui = self.state_json_actual.clone();
    }

    fn on_user_changed_state_json(&mut self) {
        let json = &serde_json::from_str(self.state_json_ui.as_str());
        if let Ok(json) = json
            && let Ok(state) =
                CreationState::try_from_json(json, Self::make_creation_state_constraints())
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
                    || self.last_rendered_preview_shells != self.preview_shells
            }
        };

        if needs_update {
            let (simulation, _) = self.to_simulation();
            self.worker_jobs
                .send(SimulationCreatorWorkerJob::GeneratePreview(
                    simulation,
                    ctx.clone(),
                    self.preview_shells,
                ))
                .unwrap();
            self.last_rendered_state = Some(self.state.clone());
        }
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

                    self.state.simulation_limits.ui(ui);

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
