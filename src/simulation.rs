use crate::collections::sliding_window::SlidingWindow;
use crate::coords::{UlamSpiralCursor, UlamSpiralPoint};
use crate::grid::{Grid, GridPoint, SquareChunker};
use crate::piece::LeaperAttacks;
use std::cmp::min;
use std::ops::{BitAnd, BitOr, BitOrAssign, BitXor};

const MAX_PLAYER_COUNT: usize = 64;

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone)]
pub struct PlayerId(u8);

impl PlayerId {
    pub fn new(id: u8) -> PlayerId {
        if (id as usize) >= MAX_PLAYER_COUNT {
            panic!("Player ID out of range");
        }
        PlayerId(id)
    }
}

impl Default for PlayerId {
    fn default() -> Self {
        PlayerId(0)
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone)]
pub struct PlayerIdSet {
    bits: u64,
}

impl PlayerIdSet {
    pub fn empty() -> PlayerIdSet {
        PlayerIdSet { bits: 0 }
    }

    pub fn full() -> PlayerIdSet {
        PlayerIdSet {
            bits: 0xFFFF_FFFF_FFFF_FFFF_u64,
        }
    }

    pub fn is_set(&self, id: PlayerId) -> bool {
        self.bits & (1u64 << id.0) != 0
    }
}

impl Default for PlayerIdSet {
    fn default() -> PlayerIdSet {
        PlayerIdSet::empty()
    }
}

impl BitAnd<PlayerIdSet> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitand(self, other: PlayerIdSet) -> PlayerIdSet {
        return PlayerIdSet {
            bits: self.bits & other.bits,
        };
    }
}

impl BitAnd<PlayerId> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitand(self, other: PlayerId) -> PlayerIdSet {
        return PlayerIdSet {
            bits: self.bits & (1u64 << other.0),
        };
    }
}

impl BitOr<PlayerIdSet> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitor(self, other: PlayerIdSet) -> PlayerIdSet {
        return PlayerIdSet {
            bits: self.bits | other.bits,
        };
    }
}

impl BitOr<PlayerId> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitor(self, other: PlayerId) -> PlayerIdSet {
        return PlayerIdSet {
            bits: self.bits | (1u64 << other.0),
        };
    }
}

impl BitOrAssign<PlayerIdSet> for PlayerIdSet {
    fn bitor_assign(&mut self, rhs: PlayerIdSet) {
        self.bits |= rhs.bits;
    }
}

impl BitOrAssign<PlayerId> for PlayerIdSet {
    fn bitor_assign(&mut self, rhs: PlayerId) {
        self.bits |= 1u64 << rhs.0;
    }
}

impl BitXor<PlayerIdSet> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitxor(self, other: PlayerIdSet) -> PlayerIdSet {
        return PlayerIdSet {
            bits: self.bits ^ other.bits,
        };
    }
}

impl BitXor<PlayerId> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitxor(self, other: PlayerId) -> PlayerIdSet {
        return PlayerIdSet {
            bits: self.bits ^ (1u64 << other.0),
        };
    }
}

pub struct Player {
    attacks: LeaperAttacks,
    id: PlayerId,
    threats: PlayerIdSet,
    cursor: UlamSpiralCursor,
}

const DEFAULT_TURNS_PER_STEP: usize = 1_000_000;
const DEFAULT_CHUNK_SIZE_POW2: u32 = 10;
const DEFAULT_SLIDING_WINDOW_CHUNK_SIZE_POW2: usize = 20;

pub struct Simulation {
    players: Vec<Player>,
    grid: Grid<PlayerId>,
    restrictions: SlidingWindow<PlayerIdSet>,

    turns_per_step: usize,
    max_turns: usize,
    max_memory: usize,
    max_distance: usize,
    simulated_turns: usize,
}

impl Simulation {
    pub fn new(max_turns: usize) -> Simulation {
        Simulation {
            players: vec![],
            grid: Grid::new(Box::new(SquareChunker::new(DEFAULT_CHUNK_SIZE_POW2))),
            restrictions: SlidingWindow::with_chunk_size_and_origin(
                DEFAULT_SLIDING_WINDOW_CHUNK_SIZE_POW2,
                0,
            ),

            turns_per_step: DEFAULT_TURNS_PER_STEP,
            max_turns,
            max_memory: usize::MAX,
            max_distance: usize::MAX,
            simulated_turns: 0,
        }
    }

    pub fn add_player(&mut self, attacks: LeaperAttacks) -> PlayerId {
        if self.players.len() >= MAX_PLAYER_COUNT {
            panic!("Too many players");
        }

        let id = PlayerId(self.players.len() as u8);

        self.players.push(Player {
            attacks,
            id,
            threats: PlayerIdSet::empty(),
            cursor: UlamSpiralCursor::new(),
        });

        id
    }

    pub fn add_player_threat(&mut self, threatening: PlayerId, threatened: PlayerId) {
        if threatening.0 as usize >= self.players.len() {
            panic!("Threatening player is out of bounds");
        }

        if threatened.0 as usize >= self.players.len() {
            panic!("Threatened player is out of bounds");
        }

        self.players[threatening.0 as usize].threats |= threatened;
    }

    fn simulate_single_turn(&mut self) {
        for player in self.players.iter_mut() {
            loop {
                let disallowed_in_this_cell =
                    self.restrictions[player.cursor.spiral_position().index()];
                if !disallowed_in_this_cell.is_set(player.id) {
                    break;
                }

                player.cursor.advance();
            }

            // We found a place we can put the piece on
            self.grid[player.cursor.grid_position()] = player.id;
            self.restrictions[player.cursor.spiral_position().index()] = PlayerIdSet::full();
            for attack_vector in player
                .attacks
                .get_attacks_from(&player.cursor.grid_position())
            {
                let u = UlamSpiralPoint::from(&attack_vector);
                self.restrictions[u.index()] |= player.threats;
            }

            // Advance after placement to remove a redundant check next turn.
            player.cursor.advance();
        }

        self.simulated_turns += 1;
    }

    fn finalize_step(&mut self) {
        let last_player = self
            .players
            .iter()
            .min_by_key(|player| player.cursor.spiral_position().index())
            .unwrap();
        let new_origin = last_player.cursor.spiral_position().index();
        self.restrictions.set_origin(new_origin);
    }

    pub fn run(&mut self) {
        loop {
            let turns_this_step = min(self.turns_per_step, self.max_turns - self.simulated_turns);
            if turns_this_step == 0 {
                break;
            }

            for t in 0..turns_this_step {
                self.simulate_single_turn()
            }

            self.finalize_step();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::grid::{GridPoint, GridVector};
    use crate::piece::LeaperAttacks;
    use crate::simulation::{PlayerIdSet, Simulation};

    #[test]
    fn single_self_attacking_knight() {
        let mut sim = Simulation::new(5);
        let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        sim.add_player_threat(p1, p1);
        sim.run();

        assert_eq!(sim.simulated_turns, 5);

        //    _  _  _  _  _
        //    _  _  1  1  _
        //    _  _ [1] 1  _
        //    _  _  _  _  _
        //    1
        
        assert_eq!(sim.grid[GridPoint::new(0, 0)], p1);
        assert_eq!(sim.grid[GridPoint::new(1, 0)], p1);
        assert_eq!(sim.grid[GridPoint::new(1, 1)], p1);
        assert_eq!(sim.grid[GridPoint::new(0, 1)], p1);
        assert_eq!(sim.grid[GridPoint::new(-2, -2)], p1);
    }

    #[test]
    fn two_knights() {
        let mut sim = Simulation::new(5);
        let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        let p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        sim.add_player_threat(p1, p2);
        sim.add_player_threat(p2, p1);
        sim.run();

        assert_eq!(sim.simulated_turns, 5);

        //             2
        //    2  2  1  1
        //    1 [1] 2  _
        //    2  _  1  _
        
        assert_eq!(sim.grid[GridPoint::new(0, 0)], p1);
        assert_eq!(sim.grid[GridPoint::new(1, 1)], p1);
        assert_eq!(sim.grid[GridPoint::new(-1, 0)], p1);
        assert_eq!(sim.grid[GridPoint::new(2, -1)], p1);
        assert_eq!(sim.grid[GridPoint::new(2, 1)], p1);

        assert_eq!(sim.grid[GridPoint::new(1, 0)], p2);
        assert_eq!(sim.grid[GridPoint::new(0, 1)], p2);
        assert_eq!(sim.grid[GridPoint::new(-1, 1)], p2);
        assert_eq!(sim.grid[GridPoint::new(-1, -1)], p2);
        assert_eq!(sim.grid[GridPoint::new(2, 2)], p2);
    }
}
