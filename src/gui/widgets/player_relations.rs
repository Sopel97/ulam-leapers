use crate::gui::widgets::widget::{JsonWidget, StatefulWidget};
use eframe::egui::{Response, Ui, Vec2};
use serde_json::{Value, json};
use std::cmp;
use std::ops::RangeInclusive;
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::game::simulation::PlayerId;
use ulam_leapers::util::blit::{Blit2D, blit_array2d};

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct PlayerRelationsInputConstraints {
    pub player_count: RangeInclusive<usize>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct PlayerRelationsInput {
    enemy_map: Array2D<bool>,
    is_symmetric: bool,
    player_count: usize,
}

impl PlayerRelationsInput {
    fn make_default_enemy_map(player_count: usize) -> Array2D<bool> {
        let mut enemy_map = Array2D::new(player_count, player_count);
        for y in 0..player_count {
            for x in 0..player_count {
                enemy_map[(x, y)] = x != y;
            }
        }
        enemy_map
    }

    pub fn new(player_count: usize) -> Self {
        let enemy_map = Self::make_default_enemy_map(player_count);

        Self {
            enemy_map,
            is_symmetric: true,
            player_count,
        }
    }

    pub fn set_player_count(&mut self, player_count: usize) {
        if self.player_count == player_count {
            return;
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
    }

    fn show_enemy_map(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        ui.vertical(|ui| {
            for y in 0..self.enemy_map.height() {
                ui.horizontal(|ui| {
                    for x in 0..self.enemy_map.width() {
                        if ui.checkbox(&mut self.enemy_map[(x, y)], "").changed()
                            && self.is_symmetric
                        {
                            self.copy_symmetrically(x, y);
                        }
                    }
                });
            }
        });
    }

    // Vec<(attacker, attacked)>
    pub fn build_attacker_attacked_pairs(&self) -> Vec<(PlayerId, PlayerId)> {
        let mut res = vec![];

        for y in 0..self.enemy_map.height() {
            for x in 0..self.enemy_map.width() {
                if self.enemy_map[(x, y)] {
                    res.push((PlayerId::new((y + 1) as u8), PlayerId::new((x + 1) as u8)));
                }
            }
        }

        res
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
            ui.horizontal(|ui| {
                ui.label("Enemies ❓").on_hover_text(
                    "Specifies which player can and cannot be placed\n\
                                    on a square attacked by a different player.\n\
                                    Player *column* fears player *row*.",
                );
                if ui.checkbox(&mut self.is_symmetric, "Symmetric").changed() && self.is_symmetric {
                    self.apply_enabled_symmetrically();
                }
            });

            self.show_enemy_map(ui);
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
            "is_enemy_map_symmetric": self.is_symmetric,
            "player_count": self.player_count,
        })
    }

    fn try_from_json(json: &Value, constraints: &PlayerRelationsInputConstraints) -> Option<Self> {
        let player_count = json["player_count"].as_u64()? as usize;
        if !constraints.player_count.contains(&player_count) {
            return None;
        }

        let mut slf = Self {
            is_symmetric: json["is_enemy_map_symmetric"].as_bool()?,
            enemy_map: Array2D::new(player_count, player_count),
            player_count,
        };

        for pair_json in json["enemy_map"].as_array()? {
            let a_pid = pair_json.get(0)?.as_u64()? as usize;
            let b_pid = pair_json.get(1)?.as_u64()? as usize;
            if !(1..=player_count).contains(&a_pid) || !(1..=player_count).contains(&b_pid) {
                return None;
            }

            let a = a_pid - 1;
            let b = b_pid - 1;
            slf.enemy_map[(b, a)] = true;
        }

        Some(slf)
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
        let input = PlayerRelationsInput::new(4);

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
        let mut input = PlayerRelationsInput::new(3);

        input.enemy_map[(1, 0)] = true; // 0 (row) attacks 1 (col) (1-based: 1 -> 2)
        input.enemy_map[(2, 0)] = true;

        let pairs = input.build_attacker_attacked_pairs();

        assert!(pairs.contains(&(pid(1), pid(2))));
        assert!(pairs.contains(&(pid(1), pid(3))));
    }

    #[test]
    fn test_copy_symmetrically() {
        let mut input = PlayerRelationsInput::new(3);

        input.enemy_map[(0, 1)] = true;
        input.copy_symmetrically(0, 1);

        assert!(input.enemy_map[(1, 0)]);
    }

    #[test]
    fn test_apply_enabled_symmetrically() {
        let mut input = PlayerRelationsInput::new(3);

        input.enemy_map[(0, 1)] = true;
        input.enemy_map[(2, 0)] = true;

        input.apply_enabled_symmetrically();

        assert!(input.enemy_map[(1, 0)]);
        assert!(input.enemy_map[(0, 2)]);
    }

    #[test]
    fn test_set_player_count_expands_correctly() {
        let mut input = PlayerRelationsInput::new(2);

        input.enemy_map[(1, 0)] = true;

        input.set_player_count(4);

        assert_eq!(input.player_count, 4);

        // old relation preserved in top-left corner
        assert!(input.enemy_map[(1, 0)]);
    }

    #[test]
    fn test_set_player_count_contracts_correctly() {
        let mut input = PlayerRelationsInput::new(4);

        input.enemy_map[(3, 2)] = true;

        input.set_player_count(2);

        assert_eq!(input.player_count, 2);

        // only 2x2 remains valid
        assert_eq!(input.enemy_map.height(), 2);
        assert_eq!(input.enemy_map.width(), 2);
    }

    #[test]
    fn test_json_roundtrip() {
        let mut input = PlayerRelationsInput::new(3);

        input.enemy_map[(1, 0)] = true;
        input.enemy_map[(2, 1)] = true;
        input.is_symmetric = true;

        let json = input.to_json();
        let restored = PlayerRelationsInput::try_from_json(&json, &constraints())
            .expect("valid json should deserialize");

        assert_eq!(restored.player_count, input.player_count);
        assert_eq!(restored.is_symmetric, input.is_symmetric);
        assert_eq!(restored.enemy_map, input.enemy_map);
    }

    #[test]
    fn test_json_rejects_invalid_player_ids() {
        let json = json!({
            "player_count": 3,
            "is_enemy_map_symmetric": true,
            "enemy_map": [
                [1, 4] // invalid player id
            ]
        });

        assert!(PlayerRelationsInput::try_from_json(&json, &constraints()).is_none());
    }

    #[test]
    fn test_json_rejects_invalid_player_ids_zero() {
        let json = json!({
            "player_count": 3,
            "is_enemy_map_symmetric": true,
            "enemy_map": [
                [1, 0] // invalid player id
            ]
        });

        assert!(PlayerRelationsInput::try_from_json(&json, &constraints()).is_none());
    }

    #[test]
    fn test_indexing_is_one_based_in_json() {
        let json = json!({
            "player_count": 3,
            "is_enemy_map_symmetric": true,
            "enemy_map": [
                [1, 2]
            ]
        });

        let input = PlayerRelationsInput::try_from_json(&json, &constraints()).unwrap();

        // stored at (1,0) internally (b, a)
        assert!(input.enemy_map[(1, 0)]);
    }

    #[test]
    fn test_no_self_enemy_by_default() {
        let input = PlayerRelationsInput::new(5);

        for i in 0..5 {
            assert!(!input.enemy_map[(i, i)]);
        }
    }

    #[test]
    fn test_symmetry_does_not_duplicate_logic() {
        let mut input = PlayerRelationsInput::new(3);

        input.enemy_map[(1, 0)] = true;
        input.enemy_map[(0, 1)] = true;

        input.apply_enabled_symmetrically();

        // still just true, no weird state corruption
        assert!(input.enemy_map[(1, 0)]);
        assert!(input.enemy_map[(0, 1)]);
    }
}
