use crate::gui::Subwindow;
use crate::gui::grid_render::default_player_colors;
use eframe::egui;
use eframe::egui::{
    Checkbox, Color32, ColorImage, Rect, ScrollArea, Slider, TextureFilter, TextureOptions,
    TextureWrapMode, Ui, Vec2, Vec2b, pos2,
};
use eframe::epaint::TextureHandle;
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
                    (xx * xs) as usize + MAX_PIECE_RANGE,
                    (yy * ys) as usize + MAX_PIECE_RANGE,
                )] = v;
                self.attack_map[(
                    (yy * ys) as usize + MAX_PIECE_RANGE,
                    (xx * xs) as usize + MAX_PIECE_RANGE,
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

    pub fn attack_offsets(&self) -> HashSet<GridVector> {
        let mut offsets = HashSet::<GridVector>::new();
        for y in 0..self.attack_map.height() {
            for x in 0..self.attack_map.width() {
                if self.attack_map[(x, y)] {
                    offsets.insert(GridVector::new(
                        (x as i32) - MAX_PIECE_RANGE as i32,
                        // Flip y because UI is rendered top to bottom while the grid's y points up.
                        -((y as i32) - MAX_PIECE_RANGE as i32),
                    ));
                }
            }
        }
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

enum SimulationCreatorWorkerJob {
    Stop,
    CancelAll,
    GeneratePreview(Simulation, egui::Context, usize),
}

enum SimulationCreatorWorkerResult {
    PreviewImage(TextureHandle),
}

pub struct SimulationCreator {
    state: CreationState,
    last_rendered_state: Option<CreationState>,

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

    fn ui(&mut self, ui: &mut Ui) {
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

                    self.show_player_configs(ui);
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

    fn show_player_configs(&mut self, ui: &mut Ui) {
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

                        ui.label("Complete shells:");
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
                });
            });
        });
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
