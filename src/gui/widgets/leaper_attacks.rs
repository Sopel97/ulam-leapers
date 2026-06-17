use crate::gui::widgets::misc::ui_layout_2d;
use crate::gui::widgets::widget::{JsonWidget, JsonWidgetError, StatefulWidget, WidgetError};
use eframe::egui;
use eframe::egui::{Checkbox, Color32, Response, Sense, Ui};
use serde_json::{json, Value};
use std::cmp;
use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::game::piece::{leaper_name_from_attack_vector, LeaperAttacks};
use ulam_leapers::math::coords::{symmetries, GridVector};
use ulam_leapers::util::blit::{blit_array2d, Blit2D};
use ulam_leapers::util::json::SerdeJsonValueExt;

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct LeaperAttacksInputConstraints {
    pub radius: RangeInclusive<usize>,
}

#[derive(Debug, Clone, Default)]
struct InternalState {
    name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LeaperAttacksInput {
    attack_map: Array2D<bool>, // NOTE: y is flipped with respect to grid coordinates!
    is_symmetric: bool,
    radius: usize,

    constraints: LeaperAttacksInputConstraints,

    internal_state: InternalState,
}

impl PartialEq for LeaperAttacksInput {
    fn eq(&self, other: &Self) -> bool {
        self.is_symmetric == other.is_symmetric
            && self.radius == other.radius
            && self.attack_map == other.attack_map
    }
}

impl Eq for LeaperAttacksInput {}

impl LeaperAttacksInput {
    pub fn new(constraints: LeaperAttacksInputConstraints) -> Self {
        let radius = *constraints.radius.start();
        let wh = radius * 2 + 1;

        Self {
            attack_map: Array2D::new(wh, wh),
            is_symmetric: true,
            radius,
            constraints,
            internal_state: InternalState::default(),
        }
    }

    pub fn set_radius(&mut self, radius: usize) -> Result<(), WidgetError> {
        if !self.constraints.radius.contains(&radius) {
            return Err(WidgetError::ConstraintViolation(format!(
                "Radius {} outside of allowed range {:?}",
                radius, self.constraints.radius
            )));
        }

        let wh = radius * 2 + 1;
        let diff = radius as i32 - self.radius as i32;
        let blit_w = cmp::min(self.attack_map.width(), wh);
        let blit_h = cmp::min(self.attack_map.height(), wh);
        let mut new_attack_map = Array2D::new(wh, wh);
        blit_array2d(
            &self.attack_map,
            &mut new_attack_map,
            &Blit2D {
                src_x: (-diff).max(0) as usize,
                src_y: (-diff).max(0) as usize,
                dst_x: diff.max(0) as usize,
                dst_y: diff.max(0) as usize,
                width: blit_w,
                height: blit_h,
            },
        );

        self.attack_map = new_attack_map;
        self.radius = radius;

        Ok(())
    }

    pub fn set_player_name(&mut self, name: String) {
        self.internal_state.name = Some(name);
    }

    pub fn radius(&self) -> usize {
        self.radius
    }

    pub fn build_leaper_attacks(&self) -> LeaperAttacks {
        LeaperAttacks::from_offsets(self.attack_offsets())
    }

    fn show_player_config_attack_map(&mut self, ui: &mut Ui) {
        let mut checkboxes = HashMap::new();
        let mut hovered_attack_offset = None;

        ui_layout_2d(
            ui,
            self.attack_map.width(),
            self.attack_map.height(),
            |ui, x, y| {
                let enabled = x != self.radius || y != self.radius;
                let checkbox_widget = Checkbox::without_text(&mut self.attack_map[(x, y)]);
                let checkbox = ui.add_enabled(enabled, checkbox_widget);

                if checkbox.changed() && self.is_symmetric {
                    self.copy_symmetrically(x, y);
                }

                let checkbox_hover_sense = ui.allocate_rect(checkbox.rect, Sense::hover());
                if checkbox_hover_sense.hovered() {
                    hovered_attack_offset = Self::index_to_attack_offset((x, y), self.radius);
                }

                checkboxes.insert((x, y), checkbox);
            },
        );

        // Highlight all symmetric attack vectors.
        if self.is_symmetric
            && let Some(hovered) = hovered_attack_offset
            && (hovered.x != 0 || hovered.y != 0)
        {
            let mut painter = ui.painter_at(ui.clip_rect());
            painter.set_opacity(0.1);
            for attack_offset in symmetries(&hovered) {
                if attack_offset == hovered {
                    continue;
                }

                let xy = Self::attack_offset_to_index(&attack_offset, self.radius)
                    .expect("All symmetric coords must be present.");
                let res = checkboxes
                    .get(&xy)
                    .expect("All symmetric checkboxes must be present.");
                painter.rect_filled(res.rect, 3, Color32::GREEN);
            }
        }
    }

    fn set_all(&mut self, enabled: bool) {
        self.attack_map.as_flat_mut_slice().fill(enabled);
        // Remember that origin cannot be set.
        self.attack_map[(self.radius, self.radius)] = false;
    }

    fn invert_all(&mut self) {
        for v in self.attack_map.as_flat_mut_slice().iter_mut() {
            *v = !*v;
        }
        // Remember that origin cannot be set.
        self.attack_map[(self.radius, self.radius)] = false;
    }

    fn set_symmetric_from_canonical(&mut self, canonical: GridVector, enabled: bool) {
        for attack_vector in symmetries(&canonical) {
            if let Some((x, y)) = Self::attack_offset_to_index(&attack_vector, self.radius) {
                self.attack_map[(x, y)] = enabled;
            }
        }
        // Remember that origin cannot be set.
        self.attack_map[(self.radius, self.radius)] = false;
    }

    /// Returns the cardinal attack vector for the selected piece
    /// or `None` if no piece was selected.
    fn show_piece_selector(ui: &mut Ui, radius: usize) -> Option<GridVector> {
        let mut clicked = None;
        ui_layout_2d(ui, radius + 1, radius, |ui, x, y| {
            let xx = x;
            let yy = y + 1;
            if xx <= yy {
                let mut button = ui.button(format!("({xx},{yy})"));

                if let Some(name) =
                    leaper_name_from_attack_vector(&GridVector::new(xx as i32, yy as i32))
                {
                    button = button.on_hover_text(name);
                }

                if button.clicked() {
                    clicked = Some(GridVector::new(xx as i32, yy as i32));
                }
            }
        });
        clicked
    }

    fn show_actions(&mut self, ui: &mut Ui) {
        ui.menu_button("Ops", |ui| {
            if ui.button("Clear").clicked() {
                self.set_all(false);
            }
            if ui.button("Fill").clicked() {
                self.set_all(true);
            }
            if ui.button("Inv").clicked() {
                self.invert_all();
            }
            ui.menu_button("Add", |ui| {
                if let Some(piece) = Self::show_piece_selector(ui, self.radius) {
                    self.set_symmetric_from_canonical(piece, true);
                }
            });
            ui.menu_button("Sub", |ui| {
                if let Some(piece) = Self::show_piece_selector(ui, self.radius) {
                    self.set_symmetric_from_canonical(piece, false);
                }
            });
        });
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
                        if let Some(name) = &self.internal_state.name {
                            ui.label(format!("{} Attacks", name));
                        } else {
                            ui.label("Attacks");
                        }

                        if ui.checkbox(&mut self.is_symmetric, "Symmetric").changed()
                            && self.is_symmetric
                        {
                            self.apply_enabled_symmetrically();
                        }

                        self.show_actions(ui);
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
            internal_state: Default::default(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct LeaperAttacksView {
    name: Option<String>,
    attack_map: Array2D<bool>, // NOTE: y is flipped with respect to grid coordinates!
}

impl LeaperAttacksView {
    pub fn with_name(name: String, attacks: &LeaperAttacks) -> Self {
        Self::new(Some(name), attacks)
    }

    pub fn new(name: Option<String>, attacks: &LeaperAttacks) -> Self {
        let attack_vectors = attacks.attack_vectors();
        let radius = attack_vectors
            .iter()
            .flat_map(|v| [v.x.unsigned_abs(), v.y.unsigned_abs()].into_iter())
            .max()
            .unwrap_or(0) as usize;
        let mut attack_map = Array2D::new(radius * 2 + 1, radius * 2 + 1);
        for v in attack_vectors {
            if let Some((x, y)) = LeaperAttacksInput::attack_offset_to_index(v, radius) {
                attack_map[(x, y)] = true;
            }
        }

        Self { name, attack_map }
    }

    fn show_attack_map(&mut self, ui: &mut Ui) {
        let radius = self.attack_map.width() / 2;
        ui_layout_2d(
            ui,
            self.attack_map.width(),
            self.attack_map.height(),
            |ui, x, y| {
                let is_middle = x == radius && y == radius;
                let checkbox_widget =
                    Checkbox::without_text(&mut self.attack_map[(x, y)]).indeterminate(is_middle);
                ui.add_enabled(false, checkbox_widget);
            },
        );
    }
}

impl StatefulWidget for LeaperAttacksView {
    fn ui(&mut self, ui: &mut Ui) -> Response {
        egui::Frame::default()
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        if let Some(name) = &self.name {
                            ui.label(format!("{} Attacks", name));
                        } else {
                            ui.label("Attacks");
                        }
                    });

                    self.show_attack_map(ui);
                });
            })
            .response
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
