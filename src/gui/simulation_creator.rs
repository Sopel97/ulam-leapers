use crate::gui::Subwindow;
use eframe::egui;
use eframe::egui::{Checkbox, Id, ScrollArea, Slider, Ui, Vec2, Vec2b};
use ulam_leapers::collections::array2d::Array2D;

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

struct State {
    player_count: usize,
    player_configs: Vec<PlayerConfigState>,
    enemy_config: EnemyConfigState,
    limits: LimitsState,
}

impl Default for State {
    fn default() -> Self {
        let mut player_configs = Vec::with_capacity(MAX_PLAYER_COUNT);
        for id in 0..MAX_PLAYER_COUNT {
            player_configs.push(PlayerConfigState::with_id(id + 1));
        }
        State {
            player_count: DEFAULT_PLAYER_COUNT,
            player_configs,
            enemy_config: Default::default(),
            limits: Default::default(),
        }
    }
}

pub struct SimulationCreator {
    state: State,
}

impl SimulationCreator {
    pub fn new() -> Self {
        Self {
            state: State::default(),
        }
    }
}

impl Subwindow for SimulationCreator {
    fn name(&self) -> String {
        "Creator".to_owned()
    }

    fn ui(&mut self, ui: &mut Ui) {
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
    }
}

impl SimulationCreator {
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
                            {
                                if self.state.enemy_config.is_enemy_map_symmetric {
                                    self.state.enemy_config.apply_enabled_symmetrically();
                                }
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
                                        {
                                            if self.state.enemy_config.is_enemy_map_symmetric {
                                                self.state.enemy_config.copy_symmetrically(x, y);
                                            }
                                        };
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

            egui::Frame::default().show(ui, |ui| {
                ui.group(|ui| {
                    ui.label("Preview: TODO");
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
                        {
                            if player_config.is_attack_map_symmetric {
                                player_config.apply_enabled_symmetrically();
                            }
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
                                {
                                    if player_config.is_attack_map_symmetric {
                                        player_config.copy_symmetrically(x, y);
                                    }
                                };
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
