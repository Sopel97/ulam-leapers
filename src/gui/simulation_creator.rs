use crate::gui::SubwindowResult::{Keep, Replace};
use crate::gui::grid_render::default_player_colors;
use crate::gui::simulation_runner::SimulationRunner;
use crate::gui::{Subwindow, SubwindowResult};
use eframe::egui;
use eframe::egui::{
    Checkbox, Color32, ColorImage, Rect, ScrollArea, Slider, TextureFilter, TextureOptions,
    TextureWrapMode, Ui, Vec2, Vec2b, pos2,
};
use eframe::epaint::TextureHandle;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::mpsc;
use std::thread::JoinHandle;
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::grid::{GridPoint, GridRect, GridVector};
use ulam_leapers::piece::LeaperAttacks;
use ulam_leapers::simulation::{PlayerId, Simulation, SimulationLimits};

const MIN_PLAYER_COUNT: usize = 1;
const DEFAULT_PLAYER_COUNT: usize = 2;
const MAX_PLAYER_COUNT: usize = 8;
const MAX_PIECE_RANGE: usize = 5;

const MIN_TURNS_M: usize = 1;
const DEFAULT_TURNS_M: usize = 1_000;
const MAX_TURNS_M: usize = 1_000_000;
const MIN_COMPLETE_SHELLS: usize = 10;
const MAX_COMPLETE_SHELLS: usize = 1_000_000;
const MIN_MEMORY_USAGE_GIB: usize = 1;
const MAX_MEMORY_USAGE_GIB: usize = 1024;

const MIN_PREVIEW_SHELLS: usize = 100;
const DEFAULT_PREVIEW_SHELLS: usize = 250;
const MAX_PREVIEW_SHELLS: usize = 1000;

#[derive(PartialEq, Clone)]
struct PlayerConfigState {
    id: usize,
    attack_map: Array2D<bool>, // NOTE: y is flipped with respect to grid coordinates!
    is_attack_map_symmetric: bool,
}

impl PlayerConfigState {
    fn attack_map_to_json(&self) -> serde_json::Value {
        json!(
            self.attack_offsets_ordered()
                .iter()
                .map(|v| {
                    json!({
                        "x": v.x,
                        "y": v.y,
                    })
                })
                .collect::<Vec<_>>()
        )
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "id": self.id,
            "attack_map": self.attack_map_to_json(),
            "is_attack_map_symmetric": self.is_attack_map_symmetric,
        })
    }

    fn try_from_json(json: &Value) -> Option<PlayerConfigState> {
        let mut slf = PlayerConfigState {
            id: json["id"].as_u64()? as usize,
            is_attack_map_symmetric: json["is_attack_map_symmetric"].as_bool()?,
            ..Default::default()
        };

        for attack_vector_json in json["attack_map"].as_array()? {
            let vec = GridVector::new(
                attack_vector_json["x"].as_i64()? as i32,
                attack_vector_json["y"].as_i64()? as i32,
            );
            let (x, y) = Self::attack_offset_to_index(&vec)?;
            slf.attack_map[(x, y)] = true;
        }

        Some(slf)
    }
}

impl PlayerConfigState {
    fn with_id(id: usize) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }

    pub fn copy_symmetrically(&mut self, x: usize, y: usize) {
        let v = self.attack_map[(x, y)];
        for xs in [-1, 1] {
            for ys in [-1, 1] {
                // Ugly because we need to translate to fix the coordinate system.
                let xx = (x as i32) - MAX_PIECE_RANGE as i32;
                let yy = (y as i32) - MAX_PIECE_RANGE as i32;
                self.attack_map[(
                    ((xx * xs) + MAX_PIECE_RANGE as i32) as usize,
                    ((yy * ys) + MAX_PIECE_RANGE as i32) as usize,
                )] = v;
                self.attack_map[(
                    ((yy * ys) + MAX_PIECE_RANGE as i32) as usize,
                    ((xx * xs) + MAX_PIECE_RANGE as i32) as usize,
                )] = v;
            }
        }
    }

    pub fn apply_enabled_symmetrically(&mut self) {
        // Some redundant work here but who cares.
        for y in 0..self.attack_map.height() {
            for x in 0..self.attack_map.width() {
                if self.attack_map[(x, y)] {
                    self.copy_symmetrically(x, y);
                }
            }
        }
    }

    fn attack_offset_to_index(attack_offset: &GridVector) -> Option<(usize, usize)> {
        let x = attack_offset.x + MAX_PIECE_RANGE as i32;
        let y = (-attack_offset.y) + MAX_PIECE_RANGE as i32;
        if x < 0 || x as usize > MAX_PIECE_RANGE * 2 || y < 0 || y as usize > MAX_PIECE_RANGE * 2 {
            return None;
        }

        Some((x as usize, y as usize))
    }

    fn index_to_attack_offset((x, y): (usize, usize)) -> Option<GridVector> {
        if x > MAX_PIECE_RANGE * 2 || y > MAX_PIECE_RANGE * 2 {
            return None;
        }

        Some(GridVector::new(
            (x as i32) - MAX_PIECE_RANGE as i32,
            // Flip y because UI is rendered top to bottom while the grid's y points up.
            -((y as i32) - MAX_PIECE_RANGE as i32),
        ))
    }

    pub fn attack_offsets(&self) -> HashSet<GridVector> {
        let mut offsets = HashSet::<GridVector>::new();
        for y in 0..self.attack_map.height() {
            for x in 0..self.attack_map.width() {
                if self.attack_map[(x, y)] {
                    offsets.insert(Self::index_to_attack_offset((x, y)).unwrap());
                }
            }
        }
        offsets
    }

    pub fn attack_offsets_ordered(&self) -> Vec<GridVector> {
        let mut offsets: Vec<_> = self.attack_offsets().into_iter().collect();
        offsets.sort();
        offsets
    }
}

impl Default for PlayerConfigState {
    fn default() -> Self {
        PlayerConfigState {
            id: 0,
            attack_map: Array2D::new(MAX_PIECE_RANGE * 2 + 1, MAX_PIECE_RANGE * 2 + 1),
            is_attack_map_symmetric: true,
        }
    }
}

#[derive(PartialEq, Clone)]
struct EnemyConfigState {
    enemy_map: Array2D<bool>,
    is_enemy_map_symmetric: bool,
}

impl EnemyConfigState {
    fn try_from_json(json: &Value) -> Option<EnemyConfigState> {
        let mut slf = EnemyConfigState {
            is_enemy_map_symmetric: json["is_enemy_map_symmetric"].as_bool()?,
            enemy_map: Array2D::new(MAX_PLAYER_COUNT, MAX_PLAYER_COUNT),
        };

        for pair_json in json["enemy_map"].as_array()? {
            let a_pid = pair_json.get(0)?.as_u64()? as usize;
            let b_pid = pair_json.get(1)?.as_u64()? as usize;
            if !(1..=MAX_PLAYER_COUNT).contains(&a_pid) || !(1..=MAX_PLAYER_COUNT).contains(&b_pid)
            {
                return None;
            }

            let a = a_pid - 1;
            let b = b_pid - 1;
            slf.enemy_map[(b, a)] = true;
        }

        Some(slf)
    }
}

impl EnemyConfigState {
    pub fn to_json(&self, player_count: usize) -> serde_json::Value {
        json!({
            "enemy_map": self.pairs(player_count).iter().map(|(a, b)| json!([a.index(), b.index()])).collect::<Vec<_>>(),
            "is_enemy_map_symmetric": self.is_enemy_map_symmetric,
        })
    }
}

impl Default for EnemyConfigState {
    fn default() -> Self {
        let mut enemy_map = Array2D::new(MAX_PLAYER_COUNT, MAX_PLAYER_COUNT);
        for y in 0..MAX_PLAYER_COUNT {
            for x in 0..MAX_PLAYER_COUNT {
                enemy_map[(x, y)] = x != y;
            }
        }

        Self {
            enemy_map,
            is_enemy_map_symmetric: true,
        }
    }
}

impl EnemyConfigState {
    // Vec<(attacker, attacked)>
    pub fn pairs(&self, player_count: usize) -> Vec<(PlayerId, PlayerId)> {
        let mut res = vec![];

        assert!(player_count <= self.enemy_map.width());
        assert!(player_count <= self.enemy_map.height());

        for y in 0..player_count {
            for x in 0..player_count {
                if self.enemy_map[(x, y)] {
                    res.push((PlayerId::new((y + 1) as u8), PlayerId::new((x + 1) as u8)));
                }
            }
        }

        res
    }

    pub fn apply_enabled_symmetrically(&mut self) {
        for y in 0..self.enemy_map.height() {
            for x in 0..self.enemy_map.width() {
                if self.enemy_map[(x, y)] {
                    self.enemy_map[(y, x)] = true;
                }
            }
        }
    }

    pub fn copy_symmetrically(&mut self, x: usize, y: usize) {
        self.enemy_map[(y, x)] = self.enemy_map[(x, y)];
    }
}

#[derive(PartialEq, Clone)]
struct LimitsState {
    memory_usage_gib: usize,
    turns_m: usize,
    complete_shells: usize,
}

impl LimitsState {
    fn try_from_json(json: &Value) -> Option<LimitsState> {
        let memory_usage = json["memory_usage"].as_u64()? as usize;
        let turns = json["turns"].as_u64()? as usize;
        let slf = LimitsState {
            memory_usage_gib: (memory_usage / 1024 / 1024 / 1024).max(1),
            turns_m: (turns / 1000 / 1000).max(1),
            complete_shells: json["complete_shells"].as_u64()? as usize,
        };

        if slf.memory_usage_gib < MIN_MEMORY_USAGE_GIB
            || slf.memory_usage_gib > MAX_MEMORY_USAGE_GIB
            || slf.turns_m < MIN_TURNS_M
            || slf.turns_m > MAX_TURNS_M
        {
            return None;
        }

        Some(slf)
    }
}

impl LimitsState {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        json!({
            "memory_usage": self.memory_usage_gib * 1024 * 1024 * 1024,
            "turns": self.turns_m * 1000 * 1000,
            "complete_shells": self.complete_shells,
        })
    }
}

impl Default for LimitsState {
    fn default() -> Self {
        Self {
            memory_usage_gib: MAX_MEMORY_USAGE_GIB,
            turns_m: DEFAULT_TURNS_M,
            complete_shells: MAX_COMPLETE_SHELLS,
        }
    }
}

#[derive(PartialEq, Clone)]
struct CreationState {
    player_count: usize,
    player_configs: Vec<PlayerConfigState>,
    enemy_config: EnemyConfigState,
    limits: LimitsState,
    preview_shells: usize,
}

impl Default for CreationState {
    fn default() -> Self {
        let mut player_configs = Vec::with_capacity(MAX_PLAYER_COUNT);
        for id in 0..MAX_PLAYER_COUNT {
            player_configs.push(PlayerConfigState::with_id(id + 1));
        }
        CreationState {
            player_count: DEFAULT_PLAYER_COUNT,
            player_configs,
            enemy_config: Default::default(),
            limits: Default::default(),
            preview_shells: DEFAULT_PREVIEW_SHELLS,
        }
    }
}

impl CreationState {
    fn to_json(&self) -> serde_json::Value {
        json!({
            "player_count": self.player_count,
            "player_configs": self.player_configs.iter().take(self.player_count).map(|p| p.to_json()).collect::<Vec<_>>(),
            "enemy_config": self.enemy_config.to_json(self.player_count),
            "limits": self.limits.to_json(),
            "preview_shells": self.preview_shells,
        })
    }

    fn try_from_json(json: &serde_json::Value) -> Option<CreationState> {
        let player_configs_array = json["player_configs"].as_array()?;
        let mut slf = CreationState {
            player_count: json["player_count"].as_u64()? as usize,
            player_configs: player_configs_array
                .iter()
                .map(PlayerConfigState::try_from_json)
                .collect::<Option<Vec<_>>>()?,
            enemy_config: EnemyConfigState::try_from_json(&json["enemy_config"])?,
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

        // We have to make sure there are actually configs allocated.
        slf.player_configs
            .resize(MAX_PLAYER_COUNT, PlayerConfigState::default());

        Some(slf)
    }
}

enum SimulationCreatorWorkerJob {
    Stop,
    CancelAll,
    GeneratePreview(Simulation, egui::Context, usize),
}

enum SimulationCreatorWorkerResult {
    PreviewImage(TextureHandle),
}

enum SimulationCreatorAction {
    Submit,
}

pub struct SimulationCreator {
    state: CreationState,
    last_rendered_state: Option<CreationState>,
    json_code_actual: String,
    json_code_ui: String,

    // TODO: maybe integrate with GridRender, though the fact that we are splitting
    //       the work into a worker thread complicates this. May not be worth it unless
    //       GridRender gains more functionality.
    preview_texture_handle: Option<TextureHandle>,

    worker: Option<JoinHandle<()>>,
    worker_jobs: mpsc::Sender<SimulationCreatorWorkerJob>,
    worker_results: mpsc::Receiver<SimulationCreatorWorkerResult>,
}

impl SimulationCreator {
    pub fn new() -> Self {
        let (job_sender, job_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();

        Self {
            state: CreationState::default(),
            last_rendered_state: None,
            json_code_actual: String::new(),
            json_code_ui: String::new(),

            preview_texture_handle: None,

            // IMPORTANT: The worker skips jobs and only focuses on the most recent one
            // TODO: clearer abstraction for this.
            worker: Some(std::thread::spawn(move || {
                let job_receiver = job_receiver;
                let result_sender = result_sender;
                loop {
                    let mut job = job_receiver.recv().unwrap();
                    if let SimulationCreatorWorkerJob::Stop = job {
                        break;
                    }

                    // Consume jobs as long as there is something newer.
                    while let Ok(newer) = job_receiver.try_recv() {
                        job = newer;

                        if let SimulationCreatorWorkerJob::Stop = job {
                            break;
                        }
                    }

                    match job {
                        SimulationCreatorWorkerJob::Stop => break,
                        SimulationCreatorWorkerJob::CancelAll => {
                            while job_receiver.try_recv().is_ok() {}
                        }
                        SimulationCreatorWorkerJob::GeneratePreview(
                            mut simulation,
                            ctx,
                            shells,
                        ) => {
                            if shells == 0 {
                                continue;
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
                            let samples: Array2D<Color32> =
                                frozen_grid.sample_range2d_map(&bounds, |v| colors[v.index()]);
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

                            result_sender
                                .send(SimulationCreatorWorkerResult::PreviewImage(handle))
                                .unwrap();

                            // The UI should now receive the preview texture, but it's reactive
                            // so we should force a repaint.
                            ctx.request_repaint();
                        }
                    }
                }
            })),
            worker_jobs: job_sender,
            worker_results: result_receiver,
        }
    }

    pub fn to_simulation(&self) -> (Simulation, SimulationLimits) {
        let mut sim = Simulation::new();

        for player_config in self.state.player_configs[..self.state.player_count].iter() {
            let attacks = player_config.attack_offsets();
            sim.add_player(LeaperAttacks::from_offsets(attacks));
        }

        let enemy_map = self.state.enemy_config.pairs(self.state.player_count);
        for (attacker, attacked) in enemy_map {
            sim.add_player_enemy(attacker, attacked);
        }

        let limits = SimulationLimits::new()
            .with_memory_limit(self.state.limits.memory_usage_gib * 1024 * 1024 * 1024)
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

        if self.json_code_ui == self.json_code_actual {
            // No user interaction, we just update the JSON code.
            self.json_code_actual = self.state.to_json().to_string();
            let json = &serde_json::from_str(self.json_code_actual.as_str()).unwrap();
            CreationState::try_from_json(json).unwrap();
            self.json_code_ui = self.json_code_actual.clone();
        } else {
            // The user has modified the JSON code, we should try parsing it.
            let json = &serde_json::from_str(self.json_code_ui.as_str());
            let mut ok = false;
            if let Ok(json) = json {
                let parsed = CreationState::try_from_json(json);
                if let Some(state) = parsed {
                    self.json_code_actual = state.to_json().to_string();
                    self.json_code_ui = self.json_code_actual.clone();
                    self.state = state;
                    ok = true;
                }
            }
            if !ok {
                self.json_code_ui = self.json_code_actual.clone();
            }
        }

        self.maybe_update_preview(ui.ctx());

        egui::Panel::left("player_setup_panel")
            .resizable(false)
            .show_inside(ui, |ui| {
                ui.vertical(|ui| {
                    ui.add(
                        Slider::new(
                            &mut self.state.player_count,
                            MIN_PLAYER_COUNT..=MAX_PLAYER_COUNT,
                        )
                        .integer()
                        .text("Player count"),
                    );

                    match self.show_player_configs(ui) {
                        Some(SimulationCreatorAction::Submit) => {
                            submit = true;
                        }
                        None => {}
                    };
                });
            });

        egui::CentralPanel::no_frame().show_inside(ui, |ui| {
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
        });

        if submit {
            let (simulation, limits) = self.to_simulation();
            Replace(Box::new(SimulationRunner::new(simulation, limits)))
        } else {
            Keep(self)
        }
    }
}

impl SimulationCreator {
    fn maybe_update_preview(&mut self, ctx: &egui::Context) {
        let needs_update = match &self.last_rendered_state {
            None => true,
            Some(last_state) => {
                // Ignore limit config because it doesn't affect the preview.
                last_state.player_count != self.state.player_count
                    || last_state.player_configs != self.state.player_configs
                    || last_state.enemy_config != self.state.enemy_config
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

    #[must_use]
    fn show_player_configs(&mut self, ui: &mut Ui) -> Option<SimulationCreatorAction> {
        let mut submit = false;

        ui.horizontal_top(|ui| {
            ScrollArea::new(Vec2b::new(false, true))
                .max_width(300.0)
                .show(ui, |ui| {
                    egui::Frame::default().show(ui, |ui| {
                        ui.vertical(|ui| {
                            // Players
                            for player_config in
                                self.state.player_configs[..self.state.player_count].iter_mut()
                            {
                                ui.group(|ui| {
                                    Self::show_player_config(ui, player_config);
                                });
                            }
                        });
                    });
                });

            egui::Frame::default().show(ui, |ui| {
                ui.vertical(|ui| {
                    // Enemy map
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("Enemies ❓").on_hover_text(
                                "Specifies which player can and cannot be placed\n\
                                    on a square attacked by a different player.\n\
                                    Player *column* fears player *row*.",
                            );
                            if ui
                                .checkbox(
                                    &mut self.state.enemy_config.is_enemy_map_symmetric,
                                    "Symmetric",
                                )
                                .changed()
                                && self.state.enemy_config.is_enemy_map_symmetric
                            {
                                self.state.enemy_config.apply_enabled_symmetrically();
                            }
                        });
                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                        ui.vertical(|ui| {
                            for y in 0..self.state.player_count {
                                ui.horizontal(|ui| {
                                    for x in 0..self.state.player_count {
                                        if ui
                                            .checkbox(
                                                &mut self.state.enemy_config.enemy_map[(x, y)],
                                                "",
                                            )
                                            .changed()
                                            && self.state.enemy_config.is_enemy_map_symmetric
                                        {
                                            self.state.enemy_config.copy_symmetrically(x, y);
                                        }
                                    }
                                });
                            }
                        });
                    });

                    // Limits
                    ui.group(|ui| {
                        ui.label("Limits:");
                        ui.label("Turns:");
                        ui.add(
                            Slider::new(&mut self.state.limits.turns_m, MIN_TURNS_M..=MAX_TURNS_M)
                                .integer()
                                .logarithmic(true)
                                .suffix(" mil"),
                        );

                        ui.label("Complete shells:");
                        ui.add(
                            Slider::new(
                                &mut self.state.limits.complete_shells,
                                MIN_COMPLETE_SHELLS..=MAX_COMPLETE_SHELLS,
                            )
                            .integer()
                            .logarithmic(true),
                        );

                        ui.label("Memory usage:");
                        ui.add(
                            Slider::new(
                                &mut self.state.limits.memory_usage_gib,
                                MIN_MEMORY_USAGE_GIB..=MAX_MEMORY_USAGE_GIB,
                            )
                            .integer()
                            .logarithmic(true)
                            .suffix(" GiB"),
                        );
                    });

                    // Actions
                    ui.group(|ui| {
                        if ui.button("Start").clicked() {
                            submit = true;
                        }
                    });

                    // Import/export strings
                    ScrollArea::both().show(ui, |ui| {
                        let line_count = self.json_code_ui.lines().count();
                        let json_code_block = egui::TextEdit::multiline(&mut self.json_code_ui)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(line_count + 1)
                            .lock_focus(true)
                            .desired_width(f32::INFINITY);
                        ui.add(json_code_block);
                    });
                });
            });
        });

        if submit {
            Some(SimulationCreatorAction::Submit)
        } else {
            None
        }
    }

    fn show_player_config(ui: &mut Ui, player_config: &mut PlayerConfigState) {
        ui.horizontal(|ui| {
            egui::Frame::default().show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(format!("P{}", player_config.id));
                });
            });

            egui::Frame::default().show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Attacks");
                        if ui
                            .checkbox(&mut player_config.is_attack_map_symmetric, "Symmetric")
                            .changed()
                            && player_config.is_attack_map_symmetric
                        {
                            player_config.apply_enabled_symmetrically();
                        }
                    });
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    for y in 0..player_config.attack_map.height() {
                        ui.horizontal(|ui| {
                            for x in 0..player_config.attack_map.width() {
                                let enabled = x != MAX_PIECE_RANGE || y != MAX_PIECE_RANGE;
                                if ui
                                    .add_enabled(
                                        enabled,
                                        Checkbox::without_text(
                                            &mut player_config.attack_map[(x, y)],
                                        ),
                                    )
                                    .changed()
                                    && player_config.is_attack_map_symmetric
                                {
                                    player_config.copy_symmetrically(x, y);
                                }
                            }
                        });
                    }
                });
            });

            // Some space.
            egui::Frame::default().show(ui, |_ui| {});
        });
    }
}
