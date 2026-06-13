use crate::gui::widgets::leaper_attacks::{LeaperAttacksInput, LeaperAttacksInputConstraints};
use crate::gui::widgets::player_relations::{
    PlayerRelationsInput, PlayerRelationsInputConstraints,
};
use crate::gui::widgets::simulation_limits::{SimulationLimitsConstraints, SimulationLimitsInput};
use crate::gui::widgets::widget::{JsonWidget, JsonWidgetError, StatefulWidget, WidgetError};
use eframe::egui;
use eframe::egui::{Color32, Response, ScrollArea, Slider, Ui, Vec2b};
use serde_json::{json, Value};
use std::ops::RangeInclusive;
use ulam_leapers::game::simulation::{Simulation, SimulationLimits};
use ulam_leapers::util::json::SerdeJsonValueExt;
use ulam_leapers::util::memory::MemSize;

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct SimulationConfigInputConstraints {
    pub attack_radius: RangeInclusive<usize>,
    pub player_count: RangeInclusive<usize>,
    pub memory_usage: RangeInclusive<MemSize>,
    pub turns: RangeInclusive<usize>,
    pub complete_shells: RangeInclusive<usize>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct SimulationConfigInput {
    player_count: usize,
    attack_radius: usize,
    player_configs: Vec<LeaperAttacksInput>,
    player_relations: PlayerRelationsInput,
    simulation_limits: SimulationLimitsInput,

    constraints: SimulationConfigInputConstraints,
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

        Ok(SimulationConfigInput {
            player_count,
            attack_radius,
            player_configs,
            player_relations,
            simulation_limits,
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
        let mut sim = Simulation::new();

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

    pub fn set_turns_limit(&mut self, turns: usize) -> Result<(), WidgetError> {
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

    /// Ignores the current value of `self.attack_radius`.
    fn set_attack_radius_ignore_current(&mut self, attack_radius: usize) -> Result<(), WidgetError> {
        if !self.constraints.attack_radius.contains(&attack_radius) {
            return Err(WidgetError::ConstraintViolation(format!(
                "Attack radius {} outside of allowed range {:?}",
                attack_radius, self.constraints.attack_radius
            )));
        }

        for player_config in self.player_configs.iter_mut() {
            player_config.set_radius(attack_radius)?;
        }

        self.attack_radius = attack_radius;

        Ok(())
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
            let mut res = LeaperAttacksInput::new(self.constraints.leaper_attacks_input_constraints());
            res.set_radius(self.attack_radius).unwrap();
            res
        });
        self.player_relations.set_player_count(player_count)?;

        Self::assign_player_names(&mut self.player_configs);

        self.player_count = player_count;

        Ok(())
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
        if !constraints.player_count.contains(&player_count) {
            return Err(WidgetError::ConstraintViolation(format!(
                "Player count {} is outside of allowed range {:?}",
                player_count, constraints.player_count
            ))
            .into());
        }

        let attack_radius = json.read_u64("attack_radius")? as usize;
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

        Ok(Self {
            player_count,
            attack_radius,
            player_configs,
            player_relations,
            simulation_limits,
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
                for (i, player_config) in player_configs.iter_mut().enumerate() {
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
                });
            });

            // Highlight the attacking player in green and the attacked player in red.
            if let Some((attacker, attacked)) = self.player_relations.hovered_attacker_attacked() {
                let attacker_rect = config_responses[attacker.index() - 1].rect;
                let attacked_rect = config_responses[attacked.index() - 1].rect;

                let mut attacker_painter = ui.painter_at(attacker_rect);
                attacker_painter.set_opacity(0.1);
                attacker_painter.rect_filled(attacker_rect, 5, Color32::GREEN);

                let mut attacked_painter = ui.painter_at(attacked_rect);
                attacked_painter.set_opacity(0.1);
                attacked_painter.rect_filled(attacked_rect, 5, Color32::RED);
            }
        });
    }

    fn show_player_count_selector(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui
                .add(
                    Slider::new(
                        &mut self.player_count,
                        self.constraints.player_count.clone(),
                    )
                    .integer()
                    .text("Player count"),
                )
                .changed()
            {
                self.on_player_count_changed()
                    .expect("The slider should be within the allowed range");
            }
            if ui
                .add(
                    Slider::new(
                        &mut self.attack_radius,
                        self.constraints.attack_radius.clone(),
                    )
                        .integer()
                        .text("Attack radius"),
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
                    self.show_player_count_selector(ui);

                    self.show_simulation_config(ui);
                });
            })
            .response
    }
}
