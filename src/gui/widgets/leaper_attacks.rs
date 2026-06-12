use crate::gui::widgets::widget::{JsonWidget, JsonWidgetError, StatefulWidget, WidgetError};
use eframe::egui;
use eframe::egui::{Checkbox, Response, Ui, Vec2};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::ops::RangeInclusive;
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::game::piece::LeaperAttacks;
use ulam_leapers::math::coords::GridVector;
use ulam_leapers::util::json::SerdeJsonValueExt;

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct LeaperAttacksInputConstraints {
    pub radius: RangeInclusive<usize>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct LeaperAttacksInput {
    attack_map: Array2D<bool>, // NOTE: y is flipped with respect to grid coordinates!
    is_symmetric: bool,
    radius: usize,

    constraints: LeaperAttacksInputConstraints,
}

impl LeaperAttacksInput {
    pub fn new(constraints: LeaperAttacksInputConstraints) -> Self {
        let radius = *constraints.radius.start();
        let wh = radius * 2 + 1;

        Self {
            attack_map: Array2D::new(wh, wh),
            is_symmetric: true,
            radius,
            constraints,
        }
    }

    pub fn set_radius(&mut self, radius: usize) -> Result<(), WidgetError> {
        if !self.constraints.radius.contains(&radius) {
            return Err(WidgetError::ConstraintViolation(format!(
                "Radius {} outside of allowed range {:?}",
                radius, self.constraints.radius
            )));
        }

        self.radius = radius;

        Ok(())
    }

    pub fn build_leaper_attacks(&self) -> LeaperAttacks {
        LeaperAttacks::from_offsets(self.attack_offsets())
    }

    fn show_player_config_attack_map(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        for y in 0..self.attack_map.height() {
            ui.horizontal(|ui| {
                for x in 0..self.attack_map.width() {
                    let enabled = x != self.radius || y != self.radius;
                    if ui
                        .add_enabled(
                            enabled,
                            Checkbox::without_text(&mut self.attack_map[(x, y)]),
                        )
                        .changed()
                        && self.is_symmetric
                    {
                        self.copy_symmetrically(x, y);
                    }
                }
            });
        }
    }

    fn attack_map_to_json(&self) -> Value {
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

    pub fn copy_symmetrically(&mut self, x: usize, y: usize) {
        let v = self.attack_map[(x, y)];
        for xs in [-1, 1] {
            for ys in [-1, 1] {
                // Ugly because we need to translate to fix the coordinate system.
                let xx = (x as i32) - self.radius as i32;
                let yy = (y as i32) - self.radius as i32;
                self.attack_map[(
                    ((xx * xs) + self.radius as i32) as usize,
                    ((yy * ys) + self.radius as i32) as usize,
                )] = v;
                self.attack_map[(
                    ((yy * ys) + self.radius as i32) as usize,
                    ((xx * xs) + self.radius as i32) as usize,
                )] = v;
            }
        }
    }

    fn apply_enabled_symmetrically(&mut self) {
        // Some redundant work here but who cares.
        for y in 0..self.attack_map.height() {
            for x in 0..self.attack_map.width() {
                if self.attack_map[(x, y)] {
                    self.copy_symmetrically(x, y);
                }
            }
        }
    }

    fn attack_offset_to_index(attack_offset: &GridVector, radius: usize) -> Option<(usize, usize)> {
        let x = attack_offset.x + radius as i32;
        let y = (-attack_offset.y) + radius as i32;
        if x < 0 || x as usize > radius * 2 || y < 0 || y as usize > radius * 2 {
            return None;
        }

        Some((x as usize, y as usize))
    }

    fn index_to_attack_offset((x, y): (usize, usize), radius: usize) -> Option<GridVector> {
        if x > radius * 2 || y > radius * 2 {
            return None;
        }

        Some(GridVector::new(
            (x as i32) - radius as i32,
            // Flip y because UI is rendered top to bottom while the grid's y points up.
            -((y as i32) - radius as i32),
        ))
    }

    fn attack_offsets(&self) -> HashSet<GridVector> {
        let mut offsets = HashSet::<GridVector>::new();
        for y in 0..self.attack_map.height() {
            for x in 0..self.attack_map.width() {
                if self.attack_map[(x, y)] {
                    offsets.insert(Self::index_to_attack_offset((x, y), self.radius).unwrap());
                }
            }
        }
        offsets
    }

    fn attack_offsets_ordered(&self) -> Vec<GridVector> {
        let mut offsets: Vec<_> = self.attack_offsets().into_iter().collect();
        offsets.sort();
        offsets
    }
}

impl StatefulWidget for LeaperAttacksInput {
    fn ui(&mut self, ui: &mut Ui) -> Response {
        egui::Frame::default()
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Attacks");
                        if ui.checkbox(&mut self.is_symmetric, "Symmetric").changed()
                            && self.is_symmetric
                        {
                            self.apply_enabled_symmetrically();
                        }
                    });

                    self.show_player_config_attack_map(ui);
                });
            })
            .response
    }
}

impl JsonWidget for LeaperAttacksInput {
    type ConstraintsType = LeaperAttacksInputConstraints;

    fn to_json(&self) -> Value {
        json!({
            "attack_map": self.attack_map_to_json(),
            "radius": self.radius,
            "is_symmetric": self.is_symmetric,
        })
    }

    fn try_from_json(
        json: &Value,
        constraints: LeaperAttacksInputConstraints,
    ) -> Result<Self, JsonWidgetError> {
        let radius = json.read_u64("radius")? as usize;
        if !constraints.radius.contains(&radius) {
            return Err(WidgetError::ConstraintViolation(format!(
                "radius {} is outside of range {:?}",
                radius, constraints.radius
            ))
            .into());
        }

        let is_symmetric = json.read_bool("is_symmetric")?;

        let mut attack_map = Array2D::new(radius * 2 + 1, radius * 2 + 1);
        for attack_vector_json in json.read_array("attack_map")? {
            let vec = GridVector::new(
                attack_vector_json.read_i64("x")? as i32,
                attack_vector_json.read_i64("y")? as i32,
            );
            if let Some((x, y)) = Self::attack_offset_to_index(&vec, radius) {
                attack_map[(x, y)] = true;
            } else {
                return Err(WidgetError::InvalidState(format!(
                    "({}, {}) attack offset beyond allowed radius.",
                    vec.x, vec.y
                ))
                .into());
            }
        }

        Ok(Self {
            is_symmetric,
            attack_map,
            radius,
            constraints,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ulam_leapers::math::coords::GridVector;

    fn vec(x: i32, y: i32) -> GridVector {
        GridVector::new(x, y)
    }

    fn constraints() -> LeaperAttacksInputConstraints {
        LeaperAttacksInputConstraints { radius: 0..=1000 }
    }

    #[test]
    fn test_roundtrip_index_conversion() {
        let radius = 2;

        for y in 0..=radius * 2 {
            for x in 0..=radius * 2 {
                let v = LeaperAttacksInput::index_to_attack_offset((x, y), radius)
                    .expect("valid index should convert");

                let (x2, y2) = LeaperAttacksInput::attack_offset_to_index(&v, radius)
                    .expect("valid offset should convert back");

                assert_eq!((x, y), (x2, y2));
            }
        }
    }

    #[test]
    fn test_center_is_identity() {
        let radius = 3;
        let center = (radius, radius);

        let v = LeaperAttacksInput::index_to_attack_offset(center, radius).unwrap();
        assert_eq!(v, vec(0, 0));

        let idx = LeaperAttacksInput::attack_offset_to_index(&vec(0, 0), radius).unwrap();
        assert_eq!(idx, center);
    }

    #[test]
    fn test_attack_map_basic_enable() {
        let mut input = LeaperAttacksInput::new(constraints());
        input.set_radius(2).unwrap();

        // Enable a single offset
        let (x, y) = LeaperAttacksInput::attack_offset_to_index(&vec(1, -1), 2).unwrap();

        input.attack_map[(x, y)] = true;

        let offsets = input.attack_offsets();

        assert!(offsets.contains(&vec(1, -1)));
    }

    #[test]
    fn test_symmetry_copies_all_quadrants() {
        let mut input = LeaperAttacksInput::new(constraints());
        input.set_radius(2).unwrap();

        // Enable one quadrant position
        let (x, y) = LeaperAttacksInput::attack_offset_to_index(&vec(1, -2), 2).unwrap();

        input.attack_map[(x, y)] = true;
        input.copy_symmetrically(x, y);

        let offsets = input.attack_offsets();

        assert_eq!(offsets.len(), 8);

        assert!(offsets.contains(&vec(1, -2)));
        assert!(offsets.contains(&vec(-1, -2)));
        assert!(offsets.contains(&vec(1, 2)));
        assert!(offsets.contains(&vec(-1, 2)));
        assert!(offsets.contains(&vec(-2, 1)));
        assert!(offsets.contains(&vec(-2, -1)));
        assert!(offsets.contains(&vec(2, 1)));
        assert!(offsets.contains(&vec(2, -1)));
    }

    #[test]
    fn test_apply_enabled_symmetrically() {
        let mut input = LeaperAttacksInput::new(constraints());
        input.set_radius(2).unwrap();

        let (x, y) = LeaperAttacksInput::attack_offset_to_index(&vec(2, -1), 2).unwrap();

        input.attack_map[(x, y)] = true;

        input.apply_enabled_symmetrically();

        let offsets = input.attack_offsets();

        assert_eq!(offsets.len(), 8);

        assert!(offsets.contains(&vec(2, -1)));
        assert!(offsets.contains(&vec(-2, -1)));
        assert!(offsets.contains(&vec(2, 1)));
        assert!(offsets.contains(&vec(-2, 1)));
        assert!(offsets.contains(&vec(-1, 2)));
        assert!(offsets.contains(&vec(-1, -2)));
        assert!(offsets.contains(&vec(1, 2)));
        assert!(offsets.contains(&vec(1, -2)));
    }

    #[test]
    fn test_json_roundtrip() {
        let mut input = LeaperAttacksInput::new(constraints());
        input.set_radius(2).unwrap();

        let (x, y) = LeaperAttacksInput::attack_offset_to_index(&vec(1, -2), 2).unwrap();
        input.attack_map[(x, y)] = true;
        input.is_symmetric = false;

        let json = input.to_json();
        let restored = LeaperAttacksInput::try_from_json(&json, constraints())
            .expect("valid json should deserialize");

        assert_eq!(restored.radius, input.radius);
        assert_eq!(restored.is_symmetric, input.is_symmetric);
        assert_eq!(restored.attack_map, input.attack_map);
    }

    #[test]
    fn test_invalid_json_fails() {
        let json = json!({
            "radius": 2,
            "is_symmetric": true,
            "attack_map": [
                {"x": 999, "y": 999}
            ]
        });

        let res = LeaperAttacksInput::try_from_json(&json, constraints());
        assert!(matches!(
            res,
            Err(JsonWidgetError::WidgetError(WidgetError::InvalidState(_)))
        ));
    }

    #[test]
    fn test_index_bounds_rejection() {
        let radius = 2;

        assert!(LeaperAttacksInput::attack_offset_to_index(&vec(10, 0), radius).is_none());
        assert!(LeaperAttacksInput::attack_offset_to_index(&vec(0, 10), radius).is_none());
        assert!(LeaperAttacksInput::attack_offset_to_index(&vec(-10, 0), radius).is_none());
    }

    #[test]
    fn test_attack_offsets_are_ordered_deterministically() {
        let mut input = LeaperAttacksInput::new(constraints());
        input.set_radius(2).unwrap();

        let points = [vec(1, -1), vec(-1, -1), vec(0, 1)];

        for p in points {
            let (x, y) = LeaperAttacksInput::attack_offset_to_index(&p, 2).unwrap();
            input.attack_map[(x, y)] = true;
        }

        let ordered = input.attack_offsets_ordered();
        let mut sorted = ordered.clone();
        sorted.sort();

        assert_eq!(ordered, sorted);
    }
}
