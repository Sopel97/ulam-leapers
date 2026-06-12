use std::ops::RangeInclusive;
use eframe::egui;
use eframe::egui::{Response, ScrollArea, Slider, Ui, Vec2b};
use serde_json::{json, Value};
use ulam_leapers::game::simulation::{Simulation, SimulationLimits};
use ulam_leapers::util::json::SerdeJsonValueExt;
use ulam_leapers::util::memory::MemSize;
use crate::gui::widgets::leaper_attacks::{LeaperAttacksInput, LeaperAttacksInputConstraints};
use crate::gui::widgets::player_relations::{PlayerRelationsInput, PlayerRelationsInputConstraints};
use crate::gui::widgets::simulation_limits::{SimulationLimitsConstraints, SimulationLimitsInput};
use crate::gui::widgets::widget::{JsonWidget, JsonWidgetError, StatefulWidget, WidgetError};

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct CreationStateConstraints {
    pub attack_radius: RangeInclusive<usize>,
    pub player_count: RangeInclusive<usize>,
    pub memory_usage: RangeInclusive<MemSize>,
    pub turns: RangeInclusive<usize>,
    pub complete_shells: RangeInclusive<usize>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct CreationState {
    player_count: usize,
    player_configs: Vec<LeaperAttacksInput>,
    player_relations: PlayerRelationsInput,
    simulation_limits: SimulationLimitsInput,

    constraints: CreationStateConstraints,
}

impl CreationState {
    pub fn new(constraints: CreationStateConstraints) -> Result<Self, WidgetError> {
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
        if !constraints.player_count.contains(&player_count) {
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

impl CreationState {
    fn show_player_configs(&mut self, ui: &mut Ui) {
        egui::Frame::default().show(ui, |ui| {
            ui.vertical(|ui| {
                // Players
                for (i, player_config) in self.player_configs.iter_mut().enumerate() {
                    ui.group(|ui| {
                        Self::show_player_config(ui, player_config, i + 1);
                    });
                }
            });
        });
    }

    fn show_all_configs(&mut self, ui: &mut Ui) {
        ui.horizontal_top(|ui| {
            ScrollArea::new(Vec2b::new(false, true))
                .max_width(300.0)
                .show(ui, |ui| {
                    self.show_player_configs(ui);
                });

            egui::Frame::default().show(ui, |ui| {
                ui.vertical(|ui| {
                    self.player_relations.ui(ui);

                    self.simulation_limits.ui(ui);
                });
            });
        });
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

impl StatefulWidget for CreationState {
    fn ui(&mut self, ui: &mut Ui) -> Response {
        ui.vertical(|ui| {
            if ui
                .add(
                    Slider::new(&mut self.player_count, self.constraints.player_count.clone())
                        .integer()
                        .text("Player count"),
                )
                .changed()
            {
                self
                    .on_player_count_changed()
                    .expect("The slider should be within the allowed range");
            }

            self.show_all_configs(ui);
        }).response
    }
}