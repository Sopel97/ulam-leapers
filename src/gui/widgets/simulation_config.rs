use std::fmt::{Debug, Display};
use crate::gui::util::format_pow2_slider_text;
use crate::gui::widgets::leaper_attacks::{LeaperAttacksInput, LeaperAttacksInputConstraints};
use crate::gui::widgets::player_relations::{
    PlayerRelationsInput, PlayerRelationsInputConstraints,
};
use crate::gui::widgets::simulation_limits::{SimulationLimitsConstraints, SimulationLimitsInput};
use crate::gui::widgets::widget::{JsonWidget, JsonWidgetError, StatefulWidget, WidgetConstraint, WidgetError};
use eframe::egui;
use eframe::egui::{Color32, Response, ScrollArea, Slider, Ui, Vec2b};
use serde_json::{json, Value};
use std::ops::{RangeBounds, RangeInclusive};
use ulam_leapers::compression::zstd::ZstdCompression;
use ulam_leapers::game::chunker::StripChunker;
use ulam_leapers::game::simulation::{Player, Simulation, SimulationLimits};
use ulam_leapers::math::pow2::Pow2;
use ulam_leapers::util::json::SerdeJsonValueExt;
use ulam_leapers::util::memory::MemSize;

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct SimulationConfigInputConstraints {
    pub attack_radius: RangeInclusive<usize>,
    pub player_count: RangeInclusive<usize>,
    pub memory_usage: RangeInclusive<MemSize>,
    pub turns: RangeInclusive<u64>,
    pub complete_shells: RangeInclusive<u64>,
    pub zstd_compression_level: RangeInclusive<i32>,
    pub chunk_strip_length: RangeInclusive<Pow2>,
    pub chunk_strip_thickness: RangeInclusive<Pow2>,
}

impl SimulationConfigInputConstraints {
    pub fn check_attack_radius(&self, attack_radius: usize) -> Result<(), WidgetError> {
        self.attack_radius.check_constraint(&attack_radius, "Attack radius")
    }

    pub fn check_player_count(&self, player_count: usize) -> Result<(), WidgetError> {
        self.player_count.check_constraint(&player_count, "Player count")
    }

    pub fn check_memory_usage(&self, memory_usage: MemSize) -> Result<(), WidgetError> {
        self.memory_usage.check_constraint(&memory_usage, "Memory usage")
    }

    pub fn check_turns(&self, turns: u64) -> Result<(), WidgetError> {
        self.turns.check_constraint(&turns, "Turns")
    }

    pub fn check_complete_shells(&self, complete_shells: u64) -> Result<(), WidgetError> {
        self.complete_shells.check_constraint(&complete_shells, "Complete shells")
    }

    pub fn check_zstd_compression_level(&self, zstd_compression_level: i32) -> Result<(), WidgetError> {
        self.zstd_compression_level.check_constraint(&zstd_compression_level, "Zstd compression level")
    }

    pub fn check_chunk_strip_length(&self, chunk_strip_length: Pow2) -> Result<(), WidgetError> {
        self.chunk_strip_length.check_constraint(&chunk_strip_length, "Strip length")
    }

    pub fn check_chunk_strip_thickness(&self, chunk_strip_thickness: Pow2) -> Result<(), WidgetError> {
        self.chunk_strip_thickness.check_constraint(&chunk_strip_thickness, "Strip thickness")
    }

    pub fn check_chunk_strip_dimensions(&self, chunk_strip_length: Pow2, chunk_strip_thickness: Pow2) -> Result<(), WidgetError> {
        self.check_chunk_strip_length(chunk_strip_length)?;
        self.check_chunk_strip_thickness(chunk_strip_thickness)?;

        if chunk_strip_thickness > chunk_strip_length {
            return Err(WidgetError::InvalidState(format!(
                "Minimum chunk strip thickness {} > minimum chunk strip length {}",
                chunk_strip_thickness,
                chunk_strip_length
            )));
        }

        Ok(())
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct SimulationConfigInput {
    player_count: usize,
    attack_radius: usize,
    player_configs: Vec<LeaperAttacksInput>,
    player_relations: PlayerRelationsInput,
    simulation_limits: SimulationLimitsInput,

    zstd_compression_level: i32,
    chunk_strip_length_pow2: u32,
    chunk_strip_thickness_pow2: u32,

    constraints: SimulationConfigInputConstraints,
}

fn pow2_range_to_u32_exponent_range(range: &RangeInclusive<Pow2>) -> RangeInclusive<u32> {
    (range.start().exponent() as u32)..=(range.end().exponent() as u32)
}

impl SimulationConfigInput {
    pub fn new(constraints: SimulationConfigInputConstraints) -> Result<Self, WidgetError> {
        let attack_radius = *constraints.attack_radius.start();
        let player_count = *constraints.player_count.start();
        let mut player_configs = vec![];
        player_configs.resize_with(player_count, || {
            LeaperAttacksInput::new(constraints.leaper_attacks_input_constraints())
        });
        Self::assign_player_names(&mut player_configs);

        let player_relations =
            PlayerRelationsInput::new(constraints.player_relations_input_constraints());

        let simulation_limits =
            SimulationLimitsInput::new(constraints.simulation_limits_input_constraints());

        let zstd_compression_level = *constraints.zstd_compression_level.start();
        let chunk_strip_length = *constraints.chunk_strip_length.start();
        let chunk_strip_thickness = *constraints.chunk_strip_thickness.start();

        constraints.check_chunk_strip_dimensions(chunk_strip_length, chunk_strip_thickness)?;

        Ok(SimulationConfigInput {
            player_count,
            attack_radius,
            player_configs,
            player_relations,
            simulation_limits,

            zstd_compression_level,
            chunk_strip_length_pow2: chunk_strip_length.exponent() as u32,
            chunk_strip_thickness_pow2: chunk_strip_thickness.exponent() as u32,

            constraints,
        })
    }

    pub fn with_players(
        players: &[Player],
        constraints: SimulationConfigInputConstraints,
    ) -> Result<Self, WidgetError> {
        let attack_radius = players
            .iter()
            .map(|player| player.attacks().radius())
            .max()
            .unwrap_or(*constraints.attack_radius.start())
            .max(*constraints.attack_radius.start());

        constraints.check_attack_radius(attack_radius)?;

        let player_count = players.len();

        constraints.check_player_count(player_count)?;

        let mut player_configs = players
            .iter()
            .map(|player| {
                LeaperAttacksInput::with_radius_and_attacks(
                    attack_radius,
                    player.attacks(),
                    constraints.leaper_attacks_input_constraints(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::assign_player_names(&mut player_configs);

        let player_relations = PlayerRelationsInput::with_players(
            players,
            constraints.player_relations_input_constraints(),
        )?;

        let simulation_limits =
            SimulationLimitsInput::new(constraints.simulation_limits_input_constraints());

        let zstd_compression_level = *constraints.zstd_compression_level.start();
        let chunk_strip_length = *constraints.chunk_strip_length.start();
        let chunk_strip_thickness = *constraints.chunk_strip_thickness.start();

        constraints.check_chunk_strip_dimensions(chunk_strip_length, chunk_strip_thickness)?;

        Ok(SimulationConfigInput {
            player_count,
            attack_radius,
            player_configs,
            player_relations,
            simulation_limits,

            zstd_compression_level,
            chunk_strip_length_pow2: chunk_strip_length.exponent() as u32,
            chunk_strip_thickness_pow2: chunk_strip_thickness.exponent() as u32,

            constraints,
        })
    }

    pub fn make_player_name(index: usize) -> String {
        format!("P{}", index + 1)
    }

    fn assign_player_names(player_configs: &mut [LeaperAttacksInput]) {
        for (i, config) in player_configs.iter_mut().enumerate() {
            config.set_player_name(Self::make_player_name(i))
        }
    }

    pub fn build_simulation(&self) -> (Simulation, SimulationLimits) {
        let chunker = StripChunker::with_strip_length_and_thickness(
            Pow2::from_exponent(self.chunk_strip_length_pow2 as u8),
            Pow2::from_exponent(self.chunk_strip_thickness_pow2 as u8),
        );
        let compression = ZstdCompression::new_with_level(self.zstd_compression_level);
        let mut sim = Simulation::with_chunker_and_compression(chunker, compression.into());

        for player_config in self.player_configs.iter() {
            sim.add_player(player_config.build_leaper_attacks());
        }

        let enemy_map = self.player_relations.build_attacker_attacked_pairs();
        for (attacker, attacked) in enemy_map {
            sim.add_player_enemy(attacker, attacked);
        }

        let limits = self.simulation_limits.build_limits();

        (sim, limits)
    }

    /// Returns whether `self` requires a preview update, assuming the last
    /// preview was generated from `old`.
    /// Simulation limits are ignored.
    pub fn requires_preview_update(&self, old: &Self) -> bool {
        self.player_count != old.player_count
            || self.player_configs != old.player_configs
            || self.player_relations != old.player_relations
    }

    pub fn set_turns_limit(&mut self, turns: u64) -> Result<(), WidgetError> {
        self.simulation_limits.set_turns(turns)
    }

    pub fn set_player_count(&mut self, player_count: usize) -> Result<(), WidgetError> {
        self.set_player_count_ignore_current(player_count)
    }

    pub fn set_attack_radius(&mut self, attack_radius: usize) -> Result<(), WidgetError> {
        self.set_attack_radius_ignore_current(attack_radius)
    }

    fn on_player_count_changed(&mut self) -> Result<(), WidgetError> {
        self.set_player_count_ignore_current(self.player_count)
    }

    fn on_attack_radius_changed(&mut self) -> Result<(), WidgetError> {
        self.set_attack_radius_ignore_current(self.attack_radius)
    }

    pub fn set_zstd_compression_level(
        &mut self,
        zstd_compression_level: i32,
    ) -> Result<(), WidgetError> {
        self.constraints.check_zstd_compression_level(zstd_compression_level)?;

        self.zstd_compression_level = zstd_compression_level;

        Ok(())
    }

    pub fn set_chunk_strip_length_and_thickness_pow2(
        &mut self,
        chunk_strip_length: Pow2,
        chunk_strip_thickness: Pow2,
    ) -> Result<(), WidgetError> {
        self.constraints.check_chunk_strip_dimensions(chunk_strip_length, chunk_strip_thickness)?;

        self.chunk_strip_length_pow2 = chunk_strip_length.exponent() as u32;
        self.chunk_strip_thickness_pow2 = chunk_strip_thickness.exponent() as u32;

        Ok(())
    }

    /// Ignores the current value of `self.attack_radius`.
    fn set_attack_radius_ignore_current(
        &mut self,
        attack_radius: usize,
    ) -> Result<(), WidgetError> {
        self.constraints.check_attack_radius(attack_radius)?;

        for player_config in self.player_configs.iter_mut() {
            player_config.set_radius(attack_radius)?;
        }

        self.attack_radius = attack_radius;

        Ok(())
    }

    /// Ignores the current value of `self.player_count`.
    fn set_player_count_ignore_current(&mut self, player_count: usize) -> Result<(), WidgetError> {
        self.constraints.check_player_count(player_count)?;

        self.player_configs.resize_with(player_count, || {
            let mut res =
                LeaperAttacksInput::new(self.constraints.leaper_attacks_input_constraints());
            res.set_radius(self.attack_radius).unwrap();
            res
        });
        self.player_relations.set_player_count(player_count)?;

        Self::assign_player_names(&mut self.player_configs);

        self.player_count = player_count;

        Ok(())
    }

    pub fn player_count(&self) -> usize {
        self.player_configs.len()
    }
}

impl SimulationConfigInputConstraints {
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

impl JsonWidget for SimulationConfigInput {
    type ConstraintsType = SimulationConfigInputConstraints;

    fn to_json(&self) -> Value {
        json!({
            "player_count": self.player_count,
            "player_configs": self.player_configs.iter().take(self.player_count).map(|p| p.to_json()).collect::<Vec<_>>(),
            "player_relations": self.player_relations.to_json(),
            "simulation_limits": self.simulation_limits.to_json(),
            "attack_radius": self.attack_radius,
            "zstd_compression_level": self.zstd_compression_level,
            "chunk_strip_length_pow2": self.chunk_strip_length_pow2,
            "chunk_strip_thickness_pow2": self.chunk_strip_thickness_pow2,
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
        constraints.check_player_count(player_count)?;

        let attack_radius = json.read_u64("attack_radius")? as usize;
        let mut player_configs = json
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
        Self::assign_player_names(&mut player_configs);

        for player_config in &player_configs {
            if player_config.radius() != attack_radius {
                return Err(WidgetError::InvalidState(format!(
                    "Attack radius in player config {} does not match global radius {}",
                    player_config.radius(),
                    attack_radius
                ))
                .into());
            }
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

        let zstd_compression_level = json.read_i64("zstd_compression_level")? as i32;
        constraints.check_zstd_compression_level(zstd_compression_level)?;

        let chunk_strip_length = Pow2::from_exponent(json.read_u64("chunk_strip_length_pow2")? as u8);
        let chunk_strip_thickness = Pow2::from_exponent(json.read_u64("chunk_strip_thickness_pow2")? as u8);
        constraints.check_chunk_strip_dimensions(chunk_strip_length, chunk_strip_thickness)?;

        Ok(Self {
            player_count,
            attack_radius,
            player_configs,
            player_relations,
            simulation_limits,

            zstd_compression_level,
            chunk_strip_length_pow2: chunk_strip_length.exponent() as u32,
            chunk_strip_thickness_pow2: chunk_strip_thickness.exponent() as u32,

            constraints,
        })
    }
}

impl SimulationConfigInput {
    fn show_player_config(ui: &mut Ui, player_config: &mut LeaperAttacksInput) {
        ui.horizontal(|ui| {
            player_config.ui(ui);
        });
    }

    fn show_player_configs(
        ui: &mut Ui,
        player_configs: &mut [LeaperAttacksInput],
    ) -> Vec<Response> {
        let mut config_responses = Vec::new();
        egui::Frame::default().show(ui, |ui| {
            ui.vertical(|ui| {
                // Players
                for player_config in player_configs.iter_mut() {
                    let res = ui
                        .group(|ui| {
                            Self::show_player_config(ui, player_config);
                        })
                        .response;
                    config_responses.push(res);
                }
            });
        });
        config_responses
    }

    fn show_advanced_simulation_config(&mut self, ui: &mut Ui) {
        ui.group(|ui| {
            ui.label("Advanced settings ❓:")
                .on_hover_text("WARNING: DO NOT TOUCH UNLESS YOU KNOW WHAT YOU'RE DOING");

            ui.label("Zstd compression level:");
            ui.add(Slider::new(
                &mut self.zstd_compression_level,
                self.constraints.zstd_compression_level.clone(),
            ));

            ui.label("Chunk strip length:");
            ui.add(
                Slider::new(
                    &mut self.chunk_strip_length_pow2,
                    pow2_range_to_u32_exponent_range(&self.constraints.chunk_strip_length),
                )
                .custom_formatter(format_pow2_slider_text),
            );

            ui.label("Chunk strip thickness:");
            let chunk_strip_thickness_range_pow2 =
                self.constraints.chunk_strip_thickness.start().exponent() as u32
                    ..=self.chunk_strip_length_pow2;
            ui.add(
                Slider::new(
                    &mut self.chunk_strip_thickness_pow2,
                    chunk_strip_thickness_range_pow2,
                )
                .custom_formatter(format_pow2_slider_text),
            );
        });
    }

    fn show_simulation_config(&mut self, ui: &mut Ui) {
        ui.horizontal_top(|ui| {
            let mut config_responses = Vec::new();

            ScrollArea::new(Vec2b::new(false, true))
                .max_width(300.0)
                .show(ui, |ui| {
                    config_responses = Self::show_player_configs(ui, &mut self.player_configs);
                });

            egui::Frame::default().show(ui, |ui| {
                ui.vertical(|ui| {
                    self.player_relations.ui(ui);

                    self.simulation_limits.ui(ui);

                    self.show_advanced_simulation_config(ui);
                });
            });

            // Highlight the attacking player in green and the attacked player in red.
            if let Some((attacker, attacked)) = self.player_relations.hovered_attacker_attacked() {
                let attacker_rect = config_responses[attacker.index() - 1].rect;
                let attacked_rect = config_responses[attacked.index() - 1].rect;

                let attacker_color;
                let attacked_color;
                if attacker == attacked || self.player_relations.is_symmetric() {
                    attacker_color = Color32::YELLOW;
                    attacked_color = Color32::YELLOW;
                } else {
                    attacker_color = Color32::GREEN;
                    attacked_color = Color32::RED;
                }

                let mut attacker_painter = ui.painter_at(attacker_rect);
                attacker_painter.set_opacity(0.1);
                attacker_painter.rect_filled(attacker_rect, 5, attacker_color);

                let mut attacked_painter = ui.painter_at(attacked_rect);
                attacked_painter.set_opacity(0.1);
                attacked_painter.rect_filled(attacked_rect, 5, attacked_color);
            }
        });
    }

    fn show_player_count_and_range_selector(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // Default slider width is 100.0, and we need it to be slightly less to fit.
            ui.style_mut().spacing.slider_width = 60.0;
            ui.label("Players:");
            if ui
                .add(
                    Slider::new(
                        &mut self.player_count,
                        self.constraints.player_count.clone(),
                    )
                    .integer(),
                )
                .changed()
            {
                self.on_player_count_changed()
                    .expect("The slider should be within the allowed range");
            }

            ui.separator();

            ui.label("Range:");
            if ui
                .add(
                    Slider::new(
                        &mut self.attack_radius,
                        self.constraints.attack_radius.clone(),
                    )
                    .integer(),
                )
                .changed()
            {
                self.on_attack_radius_changed()
                    .expect("The slider should be within the allowed range");
            }
        });
    }
}

impl StatefulWidget for SimulationConfigInput {
    fn ui(&mut self, ui: &mut Ui) -> Response {
        egui::Frame::default()
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    self.show_player_count_and_range_selector(ui);

                    self.show_simulation_config(ui);
                });
            })
            .response
    }
}
