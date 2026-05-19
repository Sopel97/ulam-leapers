use std::ops::{BitAnd, BitOr, BitXor};
use crate::piece::{ LeaperAttacks };
use crate::grid::{ Grid, GridPoint };
use crate::collections::sliding_window::SlidingWindow;

const MAX_PLAYER_COUNT: usize = 64;

struct PlayerId(u8);

impl PlayerId {
    pub fn new(id: u8) -> PlayerId {
        if (id as usize) >= MAX_PLAYER_COUNT {
            panic!("Player ID out of range");
        }
        PlayerId(id)
    }
}

struct PlayerIdSet {
    bits: u64,
}

impl PlayerIdSet {
    fn empty() -> PlayerIdSet {
        PlayerIdSet { bits: 0 }
    }

    fn full() -> PlayerIdSet {
        PlayerIdSet { bits: 0xFFFF_FFFF_FFFF_FFFF_u64 }
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
        return PlayerIdSet{bits: self.bits & other.bits};
    }
}

impl BitAnd<PlayerId> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitand(self, other: PlayerId) -> PlayerIdSet {
        return PlayerIdSet{bits: self.bits & (1u64 << other.0)};
    }
}

impl BitOr<PlayerIdSet> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitor(self, other: PlayerIdSet) -> PlayerIdSet {
        return PlayerIdSet{bits: self.bits | other.bits};
    }
}

impl BitOr<PlayerId> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitor(self, other: PlayerId) -> PlayerIdSet {
        return PlayerIdSet{bits: self.bits | (1u64 << other.0)};
    }
}

impl BitXor<PlayerIdSet> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitxor(self, other: PlayerIdSet) -> PlayerIdSet {
        return PlayerIdSet{bits: self.bits ^ other.bits};
    }
}

impl BitXor<PlayerId> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitxor(self, other: PlayerId) -> PlayerIdSet {
        return PlayerIdSet{bits: self.bits ^ (1u64 << other.0)};
    }
}

pub struct Player {
    attacks: LeaperAttacks,
    id: PlayerId,
    threats: PlayerIdSet,
    cursor: GridPoint,
}

const DEFAULT_TURNS_PER_STEP : usize = 1_000_000;

pub struct Simulation {
    players: Vec<Player>,
    grid: Grid<PlayerId>,
    restrictions: SlidingWindow<PlayerIdSet>,
    
    turns_per_step: usize,
    max_turns: usize,
    max_memory: usize,
    max_distance: usize,
    current_turn: usize,
}