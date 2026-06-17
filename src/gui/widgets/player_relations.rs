use crate::gui::widgets::misc::ui_layout_2d;
use crate::gui::widgets::widget::{JsonWidget, JsonWidgetError, StatefulWidget, WidgetError};
use eframe::egui::{Checkbox, Color32, Response, Sense, Ui};
use serde_json::{json, Value};
use std::cmp;
use std::collections::HashMap;
use std::ops::RangeInclusive;
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::game::simulation::{Player, PlayerId};
use ulam_leapers::util::blit::{blit_array2d, Blit2D};
use ulam_leapers::util::json::SerdeJsonValueExt;

const ENEMY_MAP_HELP_TEXT: &str = "Specifies which player can and cannot be placed\n\
                                    on a square attacked by a different player.\n\
                                    Player *column* fears player *row*.";

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct PlayerRelationsInputConstraints {
    pub player_count: RangeInclusive<usize>,
}

#[derive(Debug, Clone, Default)]
struct InternalState {
    hovered_attacker_attacked: Option<(PlayerId, PlayerId)>,
}

#[derive(Debug, Clone)]
pub struct PlayerRelationsInput {
    enemy_map: Array2D<bool>,
    is_symmetric: bool,
    player_count: usize,

    constraints: PlayerRelationsInputConstraints,

    internal_state: InternalState,
}

impl PartialEq for PlayerRelationsInput {
    fn eq(&self, other: &Self) -> bool {
        self.player_count == other.player_count
            && self.is_symmetric == other.is_symmetric
            && self.enemy_map == other.enemy_map
    }
}

impl Eq for PlayerRelationsInput {}

impl PlayerRelationsInput {
    pub fn new(constraints: PlayerRelationsInputConstraints) -> Self {
        let player_count = *constraints.player_count.start();
        let enemy_map = Self::make_default_enemy_map(player_count);

        Self {
            enemy_map,
            is_symmetric: true,
            player_count,
            internal_state: Default::default(),
            constraints,
        }
    }

    fn make_default_enemy_map(player_count: usize) -> Array2D<bool> {
        let mut enemy_map = Array2D::new(player_count, player_count);
        for y in 0..player_count {
            for x in 0..player_count {
                enemy_map[(x, y)] = x != y;
            }
        }
        enemy_map
    }

    pub fn is_symmetric(&self) -> bool {
        self.is_symmetric
    }

    pub fn player_count(&self) -> usize {
        self.player_count
    }

    pub fn hovered_attacker_attacked(&self) -> Option<(PlayerId, PlayerId)> {
        self.internal_state.hovered_attacker_attacked
    }

    pub fn set_player_count(&mut self, player_count: usize) -> Result<(), WidgetError> {
        if self.player_count == player_count {
            return Ok(());
        }

        if !self.constraints.player_count.contains(&player_count) {
            return Err(WidgetError::ConstraintViolation(format!(
                "Player count {} outside of allowed range {:?}",
                player_count, self.constraints.player_count
            )));
        }

        let mut new_enemy_map = Self::make_default_enemy_map(player_count);
        let common_size = cmp::min(self.player_count, player_count);
        blit_array2d(
            &self.enemy_map,
            &mut new_enemy_map,
            &Blit2D {
                src_x: 0,
                src_y: 0,
                dst_x: 0,
                dst_y: 0,
                width: common_size,
                height: common_size,
            },
        );
        self.player_count = player_count;
        self.enemy_map = new_enemy_map;

        Ok(())
    }

    fn show_enemy_map(&mut self, ui: &mut Ui) {
        self.internal_state.hovered_attacker_attacked = None;

        let mut checkboxes = HashMap::new();

        ui_layout_2d(
            ui,
            self.enemy_map.width(),
            self.enemy_map.height(),
            |ui, x, y| {
                let checkbox_widget = Checkbox::new(&mut self.enemy_map[(x, y)], "");
                let checkbox = ui.add(checkbox_widget);
                if checkbox.changed() && self.is_symmetric {
                    self.copy_symmetrically(x, y);
                }

                let checkbox_hover_sense = ui.allocate_rect(checkbox.rect, Sense::hover());
                if checkbox_hover_sense.hovered() {
                    self.internal_state.hovered_attacker_attacked =
                        Some(Self::index_to_attacker_attacked(x, y));
                }

                checkboxes.insert((x, y), checkbox);
            },
        );

        if self.is_symmetric
            && let Some(hovered) = self.internal_state.hovered_attacker_attacked
            && hovered.0 != hovered.1
        {
            let xy = Self::attacker_attacked_to_index(hovered);
            let other_checkbox = checkboxes
                .get(&(xy.1, xy.0))
                .expect("There must be a symmetric checkbox.");
            let mut painter = ui.painter_at(other_checkbox.rect);
            painter.set_opacity(0.3);
            painter.rect_filled(other_checkbox.rect, 5, Color32::YELLOW);
        }
    }

    // Vec<(attacker, attacked)>
    pub fn build_attacker_attacked_pairs(&self) -> Vec<(PlayerId, PlayerId)> {
        let mut res = vec![];

        for y in 0..self.enemy_map.height() {
            for x in 0..self.enemy_map.width() {
                if self.enemy_map[(x, y)] {
                    res.push(Self::index_to_attacker_attacked(x, y));
                }
            }
        }

        res
    }

    fn index_to_attacker_attacked(x: usize, y: usize) -> (PlayerId, PlayerId) {
        (PlayerId::new((y + 1) as u8), PlayerId::new((x + 1) as u8))
    }

    fn attacker_attacked_to_index((attacker, attacked): (PlayerId, PlayerId)) -> (usize, usize) {
        let a = attacker.index() - 1;
        let b = attacked.index() - 1;
        (b, a)
    }

    fn apply_enabled_symmetrically(&mut self) {
        for y in 0..self.enemy_map.height() {
            for x in 0..self.enemy_map.width() {
                if self.enemy_map[(x, y)] {
                    self.enemy_map[(y, x)] = true;
                }
            }
        }
    }

    fn copy_symmetrically(&mut self, x: usize, y: usize) {
        self.enemy_map[(y, x)] = self.enemy_map[(x, y)];
    }
}

impl StatefulWidget for PlayerRelationsInput {
    fn ui(&mut self, ui: &mut Ui) -> Response {
        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Enemies ❓").on_hover_text(ENEMY_MAP_HELP_TEXT);
                    if ui.checkbox(&mut self.is_symmetric, "Symmetric").changed()
                        && self.is_symmetric
                    {
                        self.apply_enabled_symmetrically();
                    }
                });

                self.show_enemy_map(ui);
            });
        })
        .response
    }
}

impl JsonWidget for PlayerRelationsInput {
    type ConstraintsType = PlayerRelationsInputConstraints;

    fn to_json(&self) -> Value {
        json!({
            "enemy_map": self.build_attacker_attacked_pairs().iter().map(|(a, b)| {
                json!([a.index(), b.index()])
            }).collect::<Vec<_>>(),
            "is_symmetric": self.is_symmetric,
            "player_count": self.player_count,
        })
    }

    fn try_from_json(
        json: &Value,
        constraints: PlayerRelationsInputConstraints,
    ) -> Result<Self, JsonWidgetError> {
        let player_count = json.read_u64("player_count")? as usize;
        if !constraints.player_count.contains(&player_count) {
            return Err(WidgetError::ConstraintViolation(format!(
                "player count {} is outside of range {:?}",
                player_count, constraints.player_count
            ))
            .into());
        }

        let is_symmetric = json.read_bool("is_symmetric")?;
        let mut enemy_map = Array2D::new(player_count, player_count);

        for pair_json in json.read_array("enemy_map")? {
            let a_pid = pair_json.read_u64(0)? as usize;
            if !(1..=player_count).contains(&a_pid) {
                return Err(WidgetError::InvalidState(format!(
                    "Player ID {} is out of range",
                    a_pid
                ))
                .into());
            }

            let b_pid = pair_json.read_u64(1)? as usize;
            if !(1..=player_count).contains(&b_pid) {
                return Err(WidgetError::InvalidState(format!(
                    "Player ID {} is out of range",
                    b_pid
                ))
                .into());
            }

            let xy = Self::attacker_attacked_to_index((
                PlayerId::new(a_pid as u8),
                PlayerId::new(b_pid as u8),
            ));
            enemy_map[xy] = true;
        }

        Ok(Self {
            player_count,
            is_symmetric,
            enemy_map,
            internal_state: Default::default(),
            constraints,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PlayerRelationsView {
    enemy_map: Array2D<bool>,
}

impl PlayerRelationsView {
    pub fn new(players: &[Player]) -> Self {
        let player_count = players.len();
        let mut enemy_map = Array2D::new(player_count, player_count);
        for (attacker, player) in players.iter().enumerate() {
            let enemies_mask = player.enemies();
            for attacked in 0..player_count {
                enemy_map[(attacker, attacked)] =
                    enemies_mask.is_set(PlayerId::new((attacked + 1) as u8));
            }
        }

        Self { enemy_map }
    }

    pub fn show_enemy_map(&mut self, ui: &mut Ui) {
        ui_layout_2d(
            ui,
            self.enemy_map.width(),
            self.enemy_map.height(),
            |ui, x, y| {
                let checkbox_widget = Checkbox::new(&mut self.enemy_map[(x, y)], "");
                ui.add_enabled(false, checkbox_widget);
            },
        );
    }
}

impl StatefulWidget for PlayerRelationsView {
    fn ui(&mut self, ui: &mut Ui) -> Response {
        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.label("Enemies ❓").on_hover_text(ENEMY_MAP_HELP_TEXT);

                self.show_enemy_map(ui);
            })
        })
        .response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ulam_leapers::game::simulation::PlayerId;

    fn pid(i: usize) -> PlayerId {
        PlayerId::new(i as u8)
    }

    fn constraints() -> PlayerRelationsInputConstraints {
        PlayerRelationsInputConstraints {
            player_count: 0..=1000,
        }
    }

    #[test]
    fn test_default_enemy_map_is_full_minus_diagonal() {
        let mut input = PlayerRelationsInput::new(constraints());
        input.set_player_count(4).unwrap();

        for y in 0..4 {
            for x in 0..4 {
                let v = input.enemy_map[(x, y)];
                if x == y {
                    assert!(!v);
                } else {
                    assert!(v);
                }
            }
        }
    }

    #[test]
    fn test_attacker_attacked_pairs_match_matrix() {
        let mut input = PlayerRelationsInput::new(constraints());
        input.set_player_count(3).unwrap();

        input.enemy_map[(1, 0)] = true; // 0 (row) attacks 1 (col) (1-based: 1 -> 2)
        input.enemy_map[(2, 0)] = true;

        let pairs = input.build_attacker_attacked_pairs();

        assert!(pairs.contains(&(pid(1), pid(2))));
        assert!(pairs.contains(&(pid(1), pid(3))));
    }

    #[test]
    fn test_copy_symmetrically() {
        let mut input = PlayerRelationsInput::new(constraints());
        input.set_player_count(3).unwrap();

        input.enemy_map[(0, 1)] = true;
        input.copy_symmetrically(0, 1);

        assert!(input.enemy_map[(1, 0)]);
    }

    #[test]
    fn test_apply_enabled_symmetrically() {
        let mut input = PlayerRelationsInput::new(constraints());
        input.set_player_count(3).unwrap();

        input.enemy_map[(0, 1)] = true;
        input.enemy_map[(2, 0)] = true;

        input.apply_enabled_symmetrically();

        assert!(input.enemy_map[(1, 0)]);
        assert!(input.enemy_map[(0, 2)]);
    }

    #[test]
    fn test_set_player_count_expands_correctly() {
        let mut input = PlayerRelationsInput::new(constraints());
        input.set_player_count(2).unwrap();

        input.enemy_map[(1, 0)] = true;

        input.set_player_count(4).unwrap();

        assert_eq!(input.player_count, 4);

        // old relation preserved in top-left corner
        assert!(input.enemy_map[(1, 0)]);
    }

    #[test]
    fn test_set_player_count_contracts_correctly() {
        let mut input = PlayerRelationsInput::new(constraints());
        input.set_player_count(4).unwrap();

        input.enemy_map[(3, 2)] = true;

        input.set_player_count(2).unwrap();

        assert_eq!(input.player_count, 2);

        // only 2x2 remains valid
        assert_eq!(input.enemy_map.height(), 2);
        assert_eq!(input.enemy_map.width(), 2);
    }

    #[test]
    fn test_json_roundtrip() {
        let mut input = PlayerRelationsInput::new(constraints());
        input.set_player_count(3).unwrap();

        input.enemy_map[(1, 0)] = true;
        input.enemy_map[(2, 1)] = true;
        input.is_symmetric = true;

        let json = input.to_json();
        let restored = PlayerRelationsInput::try_from_json(&json, constraints())
            .expect("valid json should deserialize");

        assert_eq!(restored.player_count, input.player_count);
        assert_eq!(restored.is_symmetric, input.is_symmetric);
        assert_eq!(restored.enemy_map, input.enemy_map);
    }

    #[test]
    fn test_json_rejects_invalid_player_ids() {
        let json = json!({
            "player_count": 3,
            "is_symmetric": true,
            "enemy_map": [
                [1, 4] // invalid player id
            ]
        });

        let res = PlayerRelationsInput::try_from_json(&json, constraints());
        assert!(matches!(
            res,
            Err(JsonWidgetError::WidgetError(WidgetError::InvalidState(_)))
        ));
    }

    #[test]
    fn test_json_rejects_invalid_player_ids_zero() {
        let json = json!({
            "player_count": 3,
            "is_symmetric": true,
            "enemy_map": [
                [1, 0] // invalid player id
            ]
        });

        let res = PlayerRelationsInput::try_from_json(&json, constraints());
        assert!(matches!(
            res,
            Err(JsonWidgetError::WidgetError(WidgetError::InvalidState(_)))
        ));
    }

    #[test]
    fn test_indexing_is_one_based_in_json() {
        let json = json!({
            "player_count": 3,
            "is_symmetric": true,
            "enemy_map": [
                [1, 2]
            ]
        });

        let input = PlayerRelationsInput::try_from_json(&json, constraints()).unwrap();

        // stored at (1,0) internally (b, a)
        assert!(input.enemy_map[(1, 0)]);
    }

    #[test]
    fn test_no_self_enemy_by_default() {
        let mut input = PlayerRelationsInput::new(constraints());
        input.set_player_count(5).unwrap();

        for i in 0..5 {
            assert!(!input.enemy_map[(i, i)]);
        }
    }

    #[test]
    fn test_symmetry_does_not_duplicate_logic() {
        let mut input = PlayerRelationsInput::new(constraints());
        input.set_player_count(3).unwrap();

        input.enemy_map[(1, 0)] = true;
        input.enemy_map[(0, 1)] = true;

        input.apply_enabled_symmetrically();

        // still just true, no weird state corruption
        assert!(input.enemy_map[(1, 0)]);
        assert!(input.enemy_map[(0, 1)]);
    }
}
