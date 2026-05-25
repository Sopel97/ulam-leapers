use crate::collections::sliding_window::SlidingWindow;
use crate::coords::{UlamSpiralCursor, UlamSpiralPoint};
use crate::grid::{FrozenGrid, Grid, GridPoint, SquareChunker};
use crate::piece::LeaperAttacks;
use crate::util::pow2::Pow2;
use std::cmp::min;
use std::ops::{BitAnd, BitOr, BitOrAssign, BitXor};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone)]
pub struct PlayerId(u8);

impl PlayerId {
    pub fn new(id: u8) -> PlayerId {
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
    pub fn is_player_id_allowed(id: PlayerId) -> bool {
        (id.0 as usize) < (size_of::<PlayerIdSet>() * 8)
    }

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
        PlayerIdSet {
            bits: self.bits & other.bits,
        }
    }
}

impl BitAnd<PlayerId> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitand(self, other: PlayerId) -> PlayerIdSet {
        PlayerIdSet {
            bits: self.bits & (1u64 << other.0),
        }
    }
}

impl BitOr<PlayerIdSet> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitor(self, other: PlayerIdSet) -> PlayerIdSet {
        PlayerIdSet {
            bits: self.bits | other.bits,
        }
    }
}

impl BitOr<PlayerId> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitor(self, other: PlayerId) -> PlayerIdSet {
        PlayerIdSet {
            bits: self.bits | (1u64 << other.0),
        }
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
        PlayerIdSet {
            bits: self.bits ^ other.bits,
        }
    }
}

impl BitXor<PlayerId> for PlayerIdSet {
    type Output = PlayerIdSet;

    fn bitxor(self, other: PlayerId) -> PlayerIdSet {
        PlayerIdSet {
            bits: self.bits ^ (1u64 << other.0),
        }
    }
}

pub struct Player {
    attacks: LeaperAttacks,
    id: PlayerId,
    enemies: PlayerIdSet,
    cursor: UlamSpiralCursor,
}

const DEFAULT_CHUNK_SIZE: Pow2 = Pow2::new(1024);

pub struct Simulation {
    players: Vec<Player>,
    grid: Option<Grid<PlayerId>>,
    forbiddances: SlidingWindow<PlayerIdSet>,

    simulated_turns: usize,
    is_finalized: bool,
}

#[derive(Debug, PartialEq)]
pub enum SimulationError {
    IsFinalized,
    InfiniteSimulation,
}

#[derive(Debug, PartialEq)]
pub enum SimulationLimit {
    Memory,
    Turns,
    CompleteShells,
}

pub struct SimulationLimits {
    memory: Option<usize>,
    turns: Option<usize>,
    complete_shells: Option<usize>,
}

// NOTE: Only the turn limit is accurate. Other limits are checked on best effort basis as they
//       don't control the simulation directly and may be expensive to check.
impl SimulationLimits {
    pub fn new() -> Self {
        SimulationLimits {
            memory: None,
            turns: None,
            complete_shells: None,
        }
    }

    pub fn with_memory_limit(mut self, memory: usize) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn with_turn_limit(mut self, limit: usize) -> Self {
        self.turns = Some(limit);
        self
    }

    pub fn with_complete_shell_limit(mut self, limit: usize) -> Self {
        self.complete_shells = Some(limit);
        self
    }

    pub fn any(&self) -> bool {
        self.memory.is_some() || self.turns.is_some() || self.complete_shells.is_some()
    }
}

impl Simulation {
    pub fn new() -> Simulation {
        Simulation {
            players: vec![],
            grid: Some(Grid::new(Box::new(SquareChunker::new(DEFAULT_CHUNK_SIZE)))),
            forbiddances: SlidingWindow::with_origin(0),

            simulated_turns: 0,
            is_finalized: false,
        }
    }

    pub fn empty_cell() -> PlayerId {
        PlayerId::new(0)
    }

    pub fn memory_usage(&self) -> usize {
        let grid_usage = match self.grid {
            Some(ref grid) => grid.memory_usage(),
            None => 0,
        };
        grid_usage + self.forbiddances.memory_usage()
    }

    pub fn simulated_turns(&self) -> usize {
        self.simulated_turns
    }

    pub fn is_finalized(&self) -> bool {
        self.is_finalized
    }

    pub fn add_player(&mut self, attacks: LeaperAttacks) -> PlayerId {
        if self.is_finalized {
            panic!("Cannot add players to a finalized simulation.");
        }

        if self.simulated_turns > 0 {
            panic!("Cannot add players to a running simulation.");
        }

        // ID 0 reserved for empty cell.
        let id = PlayerId((self.players.len() + 1) as u8);
        if !PlayerIdSet::is_player_id_allowed(id) {
            panic!("Simulated player with an invalid id");
        }

        self.players.push(Player {
            attacks,
            id,
            enemies: PlayerIdSet::empty(),
            cursor: UlamSpiralCursor::new(),
        });

        id
    }

    pub fn add_player_enemy(&mut self, player: PlayerId, enemy: PlayerId) {
        if self.is_finalized {
            panic!("Cannot modify player enemies in a finalized simulation.");
        }

        if self.simulated_turns > 0 {
            panic!("Cannot modify player enemies in a running simulation.");
        }

        if player.0 as usize >= self.players.len() + 1 {
            panic!("Player is out of bounds");
        }

        if enemy.0 as usize >= self.players.len() + 1 {
            panic!("Enemy player is out of bounds");
        }

        self.players[(player.0 - 1) as usize].enemies |= enemy;
    }

    pub fn add_all_pairwise_player_enemies(&mut self) {
        if self.is_finalized {
            panic!("Cannot modify player enemies in a finalized simulation.");
        }

        if self.simulated_turns > 0 {
            panic!("Cannot modify player enemies in a running simulation.");
        }

        for player in &mut self.players {
            player.enemies = PlayerIdSet::full() ^ player.id;
        }
    }

    pub fn complete_shells(&self) -> i32 {
        let min_shell = self
            .players
            .iter()
            .map(|p| p.cursor.grid_position().chebyshev_distance_from_origin())
            .min()
            .unwrap();

        min_shell
    }

    fn grid_region_past_modification(&self) -> Option<(GridPoint, GridPoint)> {
        let last_fully_simulated_shell = match self.complete_shells() {
            0 => return None,
            s => s - 1,
        };

        let min_point = GridPoint::new(-last_fully_simulated_shell, -last_fully_simulated_shell);
        let max_point = GridPoint::new(last_fully_simulated_shell, last_fully_simulated_shell);

        Some((min_point, max_point))
    }

    fn simulate_single_turn(&mut self, placements: &mut [Vec<GridPoint>]) {
        for player in self.players.iter_mut() {
            // In a lot of cases we can place the piece immediately where we currently are,
            // so special case it. Results in a significant speedup.
            let immediate_cell = &mut self.forbiddances[player.cursor.spiral_position().index()];
            if !immediate_cell.is_set(player.id) {
                *immediate_cell = PlayerIdSet::full();
            } else {
                // Skip the current element because we checked for it earlier.
                let pos = self
                    .forbiddances
                    .position_or_first_empty(player.cursor.spiral_position().index() + 1.., |x| {
                        !x.is_set(player.id)
                    });
                player.cursor.advance_to(UlamSpiralPoint::new(pos as i64));
                self.forbiddances[player.cursor.spiral_position().index()] = PlayerIdSet::full();
            }

            let point = player.cursor.grid_position();
            placements[player.id.0 as usize].push(point);
            for attack_vector in player.attacks.get_attacks_from(&point) {
                let u = UlamSpiralPoint::from(&attack_vector);
                // We don't care about cells before the origin (last player) and
                // we need to be careful not to modify them.
                if u.index() >= self.forbiddances.get_origin() {
                    self.forbiddances[u.index()] |= player.enemies;
                }
            }

            // Advance after placement to remove a redundant check next turn.
            player.cursor.advance();
        }

        self.simulated_turns += 1;
    }

    fn update_forbiddances_origin(&mut self) {
        let last_player = self
            .players
            .iter()
            .min_by_key(|player| player.cursor.spiral_position().index())
            .unwrap();
        let new_origin = last_player.cursor.spiral_position().index();
        self.forbiddances.set_origin(new_origin);
    }

    pub fn simulate(
        &mut self,
        limits: SimulationLimits,
    ) -> Result<SimulationLimit, SimulationError> {
        if self.is_finalized {
            return Err(SimulationError::IsFinalized);
        }

        if !limits.any() {
            return Err(SimulationError::InfiniteSimulation);
        }

        if self.players.is_empty() {
            return match limits.turns {
                None => Err(SimulationError::InfiniteSimulation),
                Some(t) => {
                    self.simulated_turns += t;
                    Ok(SimulationLimit::Turns)
                }
            };
        }

        if limits.memory.is_some_and(|v| self.memory_usage() >= v) {
            return Ok(SimulationLimit::Memory);
        }

        if limits
            .complete_shells
            .is_some_and(|v| self.complete_shells() as usize >= v)
        {
            return Ok(SimulationLimit::CompleteShells);
        }

        const STEP_SIZE: usize = 1024 * 16;
        const COMPRESSION_INTERVAL_STEPS: usize = 1024 * 1024 / STEP_SIZE;
        const MEMORY_USAGE_INTERVAL_STEPS: usize = 1024 * 1024 / STEP_SIZE;
        const NUM_PLACEMENT_BUFFERS: usize = 8;

        // We transfer ownership of the grid to the worker thread for the time of processing.
        let mut grid = self.grid.take().unwrap();
        let player_ids = self
            .players
            .iter()
            .map(|player| player.id)
            .collect::<Vec<_>>();
        let (job_tx, job_rx) = mpsc::channel();
        let (buffer_tx, buffer_rx) = mpsc::channel();

        enum Job {
            Place(Vec<Vec<GridPoint>>),
            Compress { min: GridPoint, max: GridPoint },
            MemoryUsage,
            Stop,
        }

        // Preallocate buffers.
        for _ in 0..NUM_PLACEMENT_BUFFERS {
            let placements: Vec<Vec<GridPoint>> = (0..self.players.len() + 1)
                .map(|i| match i {
                    0 => Vec::new(),
                    _ => Vec::with_capacity(STEP_SIZE),
                })
                .collect();
            buffer_tx.send(placements).unwrap();
        }

        let get_clear_buffer = || {
            let mut placements = buffer_rx.recv().unwrap();
            for v in placements.iter_mut() {
                v.clear();
            }
            placements
        };

        let grid_memory_usage = Arc::new(Mutex::new(0usize));
        let grid_memory_usage_worker = grid_memory_usage.clone();
        let grid_worker = thread::spawn(move || {
            loop {
                let job = job_rx.recv().unwrap();
                match job {
                    Job::Place(placements) => {
                        if placements.is_empty() {
                            break;
                        }

                        for player_id in player_ids.iter() {
                            grid.set_multiple(&placements[player_id.0 as usize], *player_id);
                        }

                        buffer_tx.send(placements).unwrap();
                    }
                    Job::Compress { min, max } => {
                        grid.freeze(&min, &max);
                    }
                    Job::MemoryUsage => {
                        let mut grid_memory_usage_worker = grid_memory_usage_worker.lock().unwrap();
                        *grid_memory_usage_worker = grid.memory_usage();
                    }
                    Job::Stop => {
                        break;
                    }
                }
            }

            grid
        });

        let mut step = 0;
        let mut turns_to_simulate = limits.turns.unwrap_or(usize::MAX);
        let mut hit_limit = SimulationLimit::Turns;
        while turns_to_simulate > 0 {
            let turns_to_simulate_this_step = min(STEP_SIZE, turns_to_simulate);

            let mut placements = get_clear_buffer();
            for _ in 0..turns_to_simulate_this_step {
                // Collect all grid placements first, then we can set them all more efficiently at the end of the step.
                self.simulate_single_turn(placements.as_mut_slice());
            }
            job_tx.send(Job::Place(placements)).unwrap();

            // Compress the grid every few steps to reduce memory usage.
            // We don't want to be doing it too often because it requires a whole chunk to be
            // outside the active area and the chunks are large; reduces redundant searches
            // for no longer active chunks.
            if step % COMPRESSION_INTERVAL_STEPS == COMPRESSION_INTERVAL_STEPS - 1 {
                if let Some((min, max)) = self.grid_region_past_modification() {
                    job_tx.send(Job::Compress { min, max }).unwrap();
                }
            }

            if limits.memory.is_some()
                && step % MEMORY_USAGE_INTERVAL_STEPS == MEMORY_USAGE_INTERVAL_STEPS - 1
            {
                // Yes, the check lags behind.
                let total = *grid_memory_usage.lock().unwrap() + self.memory_usage();
                if total >= limits.memory.unwrap() {
                    hit_limit = SimulationLimit::Memory;
                    break;
                }

                job_tx.send(Job::MemoryUsage).unwrap();
            }

            if limits
                .complete_shells
                .is_some_and(|v| self.complete_shells() as usize >= v)
            {
                hit_limit = SimulationLimit::CompleteShells;
                break;
            }

            self.update_forbiddances_origin();

            turns_to_simulate -= turns_to_simulate_this_step;
            step += 1;
        }

        // Send final message to terminate the worker thread and get the grid back.
        job_tx.send(Job::Stop).unwrap();
        self.grid = Some(grid_worker.join().unwrap());

        Ok(hit_limit)
    }

    // Freezes all chunks, deallocates simulation buffers, prohibits further simulation.
    pub fn finalize(&mut self) {
        if self.is_finalized {
            return;
        }

        match self.grid {
            Some(ref mut grid) => {
                grid.freeze_all();
            }
            None => {
                panic!("No grid");
            }
        }

        self.forbiddances.clear();
    }
    
    pub fn finalize_to_frozen_grid(mut self) -> FrozenGrid<PlayerId> {
        self.finalize();
        self.grid.unwrap().into()
    }
}

#[cfg(test)]
mod tests {
    use crate::grid::{GridPoint, GridVector};
    use crate::piece::LeaperAttacks;
    use crate::simulation::{Simulation, SimulationLimits};

    #[test]
    fn empty_cell_distinguishable_from_player() {
        let mut sim = Simulation::new();
        let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));

        assert_ne!(p1, Simulation::empty_cell());
    }

    #[test]
    fn added_players_are_different() {
        let mut sim = Simulation::new();
        let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        let p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));

        assert_ne!(p1, p2);
    }

    #[test]
    fn empty_simulation_works() {
        let mut sim = Simulation::new();
        sim.simulate(SimulationLimits::new().with_turn_limit(100))
            .unwrap();
        assert_eq!(sim.simulated_turns(), 100);
    }

    #[test]
    fn single_self_attacking_knight() {
        let mut sim = Simulation::new();
        let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        sim.add_player_enemy(p1, p1);
        sim.simulate(SimulationLimits::new().with_turn_limit(5))
            .unwrap();

        assert_eq!(sim.simulated_turns, 5);

        //    _  _  _  _  _
        //    _  _  1  1  _
        //    _  _ [1] 1  _
        //    _  _  _  _  _
        //    1

        let grid = match &sim.grid {
            Some(grid) => grid,
            _ => panic!("No grid"),
        };
        assert_eq!(grid[GridPoint::new(0, 0)], p1);
        assert_eq!(grid[GridPoint::new(1, 0)], p1);
        assert_eq!(grid[GridPoint::new(1, 1)], p1);
        assert_eq!(grid[GridPoint::new(0, 1)], p1);
        assert_eq!(grid[GridPoint::new(-2, -2)], p1);
    }

    #[test]
    fn two_knights() {
        let mut sim = Simulation::new();
        let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        let p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        sim.add_all_pairwise_player_enemies();
        sim.simulate(SimulationLimits::new().with_turn_limit(5))
            .unwrap();

        assert_eq!(sim.simulated_turns, 5);

        //    2  2  1  1
        //    1 [1] 2  2
        //    2  _  _  1

        let grid = match &sim.grid {
            Some(grid) => grid,
            _ => panic!("No grid"),
        };
        assert_eq!(grid[GridPoint::new(0, 0)], p1);
        assert_eq!(grid[GridPoint::new(1, 1)], p1);
        assert_eq!(grid[GridPoint::new(-1, 0)], p1);
        assert_eq!(grid[GridPoint::new(2, -1)], p1);
        assert_eq!(grid[GridPoint::new(2, 1)], p1);

        assert_eq!(grid[GridPoint::new(1, 0)], p2);
        assert_eq!(grid[GridPoint::new(0, 1)], p2);
        assert_eq!(grid[GridPoint::new(-1, 1)], p2);
        assert_eq!(grid[GridPoint::new(-1, -1)], p2);
        assert_eq!(grid[GridPoint::new(2, 0)], p2);
    }

    #[test]
    fn simulation_is_resumable() {
        let mut sim = Simulation::new();
        let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        let p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        sim.add_all_pairwise_player_enemies();

        sim.simulate(SimulationLimits::new().with_turn_limit(2))
            .unwrap();

        assert_eq!(sim.simulated_turns, 2);

        //    2  1
        //   [1] 2

        let grid = match &sim.grid {
            Some(grid) => grid,
            _ => panic!("No grid"),
        };
        assert_eq!(grid[GridPoint::new(0, 0)], p1);
        assert_eq!(grid[GridPoint::new(1, 1)], p1);

        assert_eq!(grid[GridPoint::new(1, 0)], p2);
        assert_eq!(grid[GridPoint::new(0, 1)], p2);

        sim.simulate(SimulationLimits::new().with_turn_limit(3))
            .unwrap();

        assert_eq!(sim.simulated_turns, 5);

        //    2  2  1  1
        //    1 [1] 2  2
        //    2  _  _  1

        let grid = match &sim.grid {
            Some(grid) => grid,
            _ => panic!("No grid"),
        };
        assert_eq!(grid[GridPoint::new(-1, 0)], p1);
        assert_eq!(grid[GridPoint::new(2, -1)], p1);
        assert_eq!(grid[GridPoint::new(2, 1)], p1);

        assert_eq!(grid[GridPoint::new(-1, 1)], p2);
        assert_eq!(grid[GridPoint::new(-1, -1)], p2);
        assert_eq!(grid[GridPoint::new(2, 0)], p2);
    }
}
