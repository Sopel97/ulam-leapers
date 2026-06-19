use crate::gui::widgets::widget::{JsonWidget, JsonWidgetError, StatefulWidget, WidgetError};
use eframe::egui::{Response, Slider, Ui};
use serde_json::{Value, json};
use std::ops::RangeInclusive;
use ulam_leapers::game::simulation::SimulationLimits;
use ulam_leapers::util::constraint::Constraint;
use ulam_leapers::util::json::SerdeJsonValueExt;
use ulam_leapers::util::memory::MemSize;

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct SimulationLimitsConstraints {
    pub memory_usage: RangeInclusive<MemSize>,
    pub turns: RangeInclusive<u64>,
    pub complete_shells: RangeInclusive<u64>,
}

impl SimulationLimitsConstraints {
    pub fn check_memory_usage(&self, memory_usage: MemSize) -> Result<(), WidgetError> {
        self.memory_usage
            .check_constraint(&memory_usage, "Memory usage")
            .map_err(WidgetError::from)
    }

    pub fn check_turns(&self, turns: u64) -> Result<(), WidgetError> {
        self.turns
            .check_constraint(&turns, "Turns")
            .map_err(WidgetError::from)
    }

    pub fn check_complete_shells(&self, complete_shells: u64) -> Result<(), WidgetError> {
        self.complete_shells
            .check_constraint(&complete_shells, "Complete shells")
            .map_err(WidgetError::from)
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct SimulationLimitsInput {
    memory_usage: usize,
    turns: u64,
    complete_shells: u64,

    constraints: SimulationLimitsConstraints,
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

    pub fn build_limits(&self) -> SimulationLimits {
        SimulationLimits::new()
            .with_memory_limit(MemSize::b(self.memory_usage))
            .with_turn_limit(self.turns)
            .with_complete_shell_limit(self.complete_shells)
    }

    pub fn set_memory_usage(&mut self, memory_usage: MemSize) -> Result<(), WidgetError> {
        self.constraints.check_memory_usage(memory_usage)?;

        self.memory_usage = memory_usage.bytes();

        Ok(())
    }

    pub fn set_turns(&mut self, turns: u64) -> Result<(), WidgetError> {
        self.constraints.check_turns(turns)?;

        self.turns = turns;

        Ok(())
    }

    pub fn set_complete_shells(&mut self, complete_shells: u64) -> Result<(), WidgetError> {
        self.constraints.check_complete_shells(complete_shells)?;

        self.complete_shells = complete_shells;

        Ok(())
    }
}

impl StatefulWidget for SimulationLimitsInput {
    fn ui(&mut self, ui: &mut Ui) -> Response {
        ui.group(|ui| {
            ui.label("Limits:");
            ui.label("Turns:");
            ui.add(
                Slider::new(&mut self.turns, self.constraints.turns.clone())
                    .integer()
                    .logarithmic(true)
                    .custom_formatter(|s, _| {
                        if s < 1e9 {
                            format!("{:} M", s / 1e6)
                        } else {
                            format!("{:} G", s / 1e9)
                        }
                    }),
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
        })
        .response
    }
}

impl JsonWidget for SimulationLimitsInput {
    type ConstraintsType = SimulationLimitsConstraints;

    fn to_json(&self) -> Value {
        json!({
            "memory_usage": self.memory_usage,
            "turns": self.turns,
            "complete_shells": self.complete_shells,
        })
    }

    fn try_from_json(
        json: &Value,
        constraints: SimulationLimitsConstraints,
    ) -> Result<Self, JsonWidgetError> {
        let memory_usage = json.read_u64("memory_usage")? as usize;
        constraints.check_memory_usage(MemSize::b(memory_usage))?;

        let turns = json.read_u64("turns")?;
        constraints.check_turns(turns)?;

        let complete_shells = json.read_u64("complete_shells")?;
        constraints.check_complete_shells(complete_shells)?;

        Ok(Self {
            memory_usage,
            turns,
            complete_shells,
            constraints: constraints.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ulam_leapers::util::memory::MemSize;

    fn constraints() -> SimulationLimitsConstraints {
        SimulationLimitsConstraints {
            memory_usage: MemSize::b(100)..=MemSize::b(1000),
            turns: 10..=100,
            complete_shells: 1..=5,
        }
    }

    #[test]
    fn test_new_initializes_to_max_bounds() {
        let c = constraints();
        let input = SimulationLimitsInput::new(c.clone());

        assert_eq!(input.memory_usage, 1000);
        assert_eq!(input.turns, 100);
        assert_eq!(input.complete_shells, 5);
    }

    #[test]
    fn test_try_set_memory_usage_valid() {
        let mut input = SimulationLimitsInput::new(constraints());

        let result = input.set_memory_usage(MemSize::b(500));

        assert!(result.is_ok());
        assert_eq!(input.memory_usage, 500);
    }

    #[test]
    fn test_try_set_memory_usage_invalid() {
        let mut input = SimulationLimitsInput::new(constraints());

        let result = input.set_memory_usage(MemSize::b(9999));

        assert!(matches!(result, Err(WidgetError::ConstraintViolation(_))));
    }

    #[test]
    fn test_try_set_turns_valid() {
        let mut input = SimulationLimitsInput::new(constraints());

        let result = input.set_turns(50);

        assert!(result.is_ok());
        assert_eq!(input.turns, 50);
    }

    #[test]
    fn test_try_set_turns_invalid() {
        let mut input = SimulationLimitsInput::new(constraints());

        let result = input.set_turns(999);

        assert!(matches!(result, Err(WidgetError::ConstraintViolation(_))));
    }

    #[test]
    fn test_try_set_complete_shells_valid() {
        let mut input = SimulationLimitsInput::new(constraints());

        let result = input.set_complete_shells(3);

        assert!(result.is_ok());
        assert_eq!(input.complete_shells, 3);
    }

    #[test]
    fn test_try_set_complete_shells_invalid() {
        let mut input = SimulationLimitsInput::new(constraints());

        let result = input.set_complete_shells(999);

        assert!(matches!(result, Err(WidgetError::ConstraintViolation(_))));
    }

    #[test]
    fn test_build_limits_matches_input() {
        let mut input = SimulationLimitsInput::new(constraints());

        input.set_memory_usage(MemSize::b(250)).unwrap();
        input.set_turns(42).unwrap();
        input.set_complete_shells(2).unwrap();

        let limits = input.build_limits();

        assert_eq!(limits.memory(), Some(MemSize::b(250)));
        assert_eq!(limits.turns(), Some(42));
        assert_eq!(limits.complete_shells(), Some(2));
    }

    #[test]
    fn test_json_roundtrip_valid() {
        let c = constraints();
        let mut input = SimulationLimitsInput::new(c.clone());

        input.set_memory_usage(MemSize::b(321)).unwrap();
        input.set_turns(77).unwrap();
        input.set_complete_shells(4).unwrap();

        let json = input.to_json();

        let restored =
            SimulationLimitsInput::try_from_json(&json, c).expect("valid json should deserialize");

        assert_eq!(restored.memory_usage, 321);
        assert_eq!(restored.turns, 77);
        assert_eq!(restored.complete_shells, 4);
    }

    #[test]
    fn test_json_rejects_invalid_memory() {
        let json = json!({
            "memory_usage": 99999, // out of range
            "turns": 50,
            "complete_shells": 2,
        });

        let res = SimulationLimitsInput::try_from_json(&json, constraints());
        assert!(matches!(
            res,
            Err(JsonWidgetError::WidgetError(
                WidgetError::ConstraintViolation(_)
            ))
        ));
    }

    #[test]
    fn test_json_rejects_invalid_turns() {
        let json = json!({
            "memory_usage": 200,
            "turns": 9999,
            "complete_shells": 2,
        });

        let res = SimulationLimitsInput::try_from_json(&json, constraints());
        assert!(matches!(
            res,
            Err(JsonWidgetError::WidgetError(
                WidgetError::ConstraintViolation(_)
            ))
        ));
    }

    #[test]
    fn test_json_rejects_invalid_shells() {
        let json = json!({
            "memory_usage": 200,
            "turns": 50,
            "complete_shells": 9999,
        });

        let res = SimulationLimitsInput::try_from_json(&json, constraints());
        assert!(matches!(
            res,
            Err(JsonWidgetError::WidgetError(
                WidgetError::ConstraintViolation(_)
            ))
        ));
    }

    #[test]
    fn test_constraints_are_enforced_after_deserialization() {
        let json = json!({
            "memory_usage": 500,
            "turns": 50,
            "complete_shells": 2,
        });

        let mut input =
            SimulationLimitsInput::try_from_json(&json, constraints()).expect("valid json");

        // Now try to violate constraints via setters
        assert!(input.set_turns(9999).is_err());
        assert!(input.set_memory_usage(MemSize::b(9999)).is_err());
        assert!(input.set_complete_shells(9999).is_err());
    }

    #[test]
    fn test_memory_bounds_are_respected_in_new() {
        let input = SimulationLimitsInput::new(constraints());

        assert!(input.memory_usage >= 100);
        assert!(input.memory_usage <= 1000);
    }
}
