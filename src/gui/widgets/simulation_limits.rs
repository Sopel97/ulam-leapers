use eframe::egui::{Slider, Ui};
use serde_json::{Value, json};
use std::ops::RangeInclusive;
use ulam_leapers::game::simulation::SimulationLimits;
use ulam_leapers::util::memory::MemSize;

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct SimulationLimitsConstraints {
    pub memory_usage: RangeInclusive<MemSize>,
    pub turns: RangeInclusive<usize>,
    pub complete_shells: RangeInclusive<usize>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct SimulationLimitsInput {
    memory_usage: usize,
    turns: usize,
    complete_shells: usize,

    // The following state is not serialized, it's enforced by the user.
    // It just so happens that we need to keep it stored for the slider ranges.
    constraints: SimulationLimitsConstraints,
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum SimulationLimitsInputError {
    ConstraintViolation,
}

impl SimulationLimitsInput {
    pub fn new(constraints: SimulationLimitsConstraints) -> Self {
        Self {
            memory_usage: constraints.memory_usage.end().bytes(),
            turns: *constraints.turns.end(),
            complete_shells: *constraints.complete_shells.end(),
            constraints,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        ui.group(|ui| {
            ui.label("Limits:");
            ui.label("Turns:");
            ui.add(
                Slider::new(&mut self.turns, self.constraints.turns.clone())
                    .integer()
                    .logarithmic(true),
            );

            ui.label("Complete shells:");
            ui.add(
                Slider::new(
                    &mut self.complete_shells,
                    self.constraints.complete_shells.clone(),
                )
                .integer()
                .logarithmic(true),
            );

            let memory_usage_range = self.constraints.memory_usage.start().bytes()
                ..=self.constraints.memory_usage.end().bytes();
            ui.label("Memory usage:");
            ui.add(
                Slider::new(&mut self.memory_usage, memory_usage_range)
                    .integer()
                    .logarithmic(true)
                    .custom_formatter(|s, _| MemSize::b(s as usize).display().si().to_string()),
            );
        });
    }

    pub fn build_limits(&self) -> SimulationLimits {
        SimulationLimits::new()
            .with_memory_limit(MemSize::b(self.memory_usage))
            .with_turn_limit(self.turns)
            .with_complete_shell_limit(self.complete_shells)
    }

    pub fn try_set_memory_usage(
        &mut self,
        memory_usage: MemSize,
    ) -> Result<(), SimulationLimitsInputError> {
        if self.constraints.memory_usage.contains(&memory_usage) {
            self.memory_usage = memory_usage.bytes();
            Ok(())
        } else {
            Err(SimulationLimitsInputError::ConstraintViolation)
        }
    }

    pub fn try_set_turns(&mut self, turns: usize) -> Result<(), SimulationLimitsInputError> {
        if self.constraints.turns.contains(&turns) {
            self.turns = turns;
            Ok(())
        } else {
            Err(SimulationLimitsInputError::ConstraintViolation)
        }
    }

    pub fn try_set_complete_shells(
        &mut self,
        complete_shells: usize,
    ) -> Result<(), SimulationLimitsInputError> {
        if self.constraints.complete_shells.contains(&complete_shells) {
            self.complete_shells = complete_shells;
            Ok(())
        } else {
            Err(SimulationLimitsInputError::ConstraintViolation)
        }
    }

    pub fn try_from_json(json: &Value, constraints: SimulationLimitsConstraints) -> Option<Self> {
        let memory_usage = json["memory_usage"].as_u64()? as usize;
        let turns = json["turns"].as_u64()? as usize;
        let complete_shells = json["complete_shells"].as_u64()? as usize;

        if !constraints.memory_usage.contains(&MemSize::b(memory_usage))
            || !constraints.turns.contains(&turns)
            || !constraints.complete_shells.contains(&complete_shells)
        {
            return None;
        }

        Some(Self {
            memory_usage,
            turns,
            complete_shells,
            constraints,
        })
    }

    pub fn to_json(&self) -> Value {
        json!({
            "memory_usage": self.memory_usage,
            "turns": self.turns,
            "complete_shells": self.complete_shells,
        })
    }
}
