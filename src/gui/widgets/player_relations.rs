use eframe::egui::{Response, Ui, Vec2};
use serde_json::{Value, json};
use std::cmp;
use ulam_leapers::collections::array2d::Array2D;
use ulam_leapers::game::simulation::PlayerId;
use ulam_leapers::util::blit::{Blit2D, blit_array2d};

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

    pub fn ui(&mut self, ui: &mut Ui) -> Response {
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

    pub fn to_json(&self, player_count: usize) -> Value {
        json!({
            "enemy_map": self.build_attacker_attacked_pairs().iter().map(|(a, b)| json!([a.index(), b.index()])).collect::<Vec<_>>(),
            "is_enemy_map_symmetric": self.is_symmetric,
            "player_count": self.player_count,
        })
    }

    pub fn try_from_json(json: &Value) -> Option<Self> {
        let player_count = json["player_count"].as_u64()? as usize;
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
