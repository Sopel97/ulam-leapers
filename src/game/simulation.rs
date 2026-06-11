use crate::collections::sliding_window::SlidingWindow;
use crate::compression::zstd::ZstdCompression;
use crate::game::chunker::SquareChunker;
use crate::game::grid::{FrozenGrid, Grid};
use crate::game::piece::LeaperAttacks;
use crate::io::{ReadFrom, WriteTo};
use crate::math::coords::GridPoint;
use crate::math::pow2::Pow2;
use crate::math::rect::GridRect;
use crate::math::ulam::{UlamSpiralCursor, UlamSpiralPoint};
use crate::util::memory::MemSize;
use std::cmp::{max, min};
use std::io::{ErrorKind, Read, Write};
use std::ops::{BitAnd, BitOr, BitOrAssign, BitXor};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

#[derive(Debug, Hash, Eq, PartialEq, Copy, Clone, Default, PartialOrd, Ord)]
pub struct PlayerId(u8);

impl PlayerId {
    pub fn new(id: u8) -> PlayerId {
        PlayerId(id)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Hash, Eq, PartialEq, Copy, Clone)]
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

    pub fn highest_player_id(&self) -> PlayerId {
        PlayerId::new((63 - self.bits.leading_zeros() as i32).max(0) as u8)
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

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Player {
    attacks: LeaperAttacks,
    id: PlayerId,
    enemies: PlayerIdSet,
    cursor: UlamSpiralCursor,
}

const DEFAULT_CHUNK_SIZE: Pow2 = Pow2::from_exponent(10);

pub trait Game {
    fn players(&self) -> &Vec<Player>;
    fn complete_turns(&self) -> usize;

    fn complete_shells(&self) -> u32 {
        self.players()
            .iter()
            .map(|p| p.cursor.grid_position().chebyshev_distance_to_origin())
            .min()
            .unwrap()
    }

    fn farthest_player_spiral_position(&self) -> UlamSpiralPoint {
        self.players()
            .iter()
            .map(|p| p.cursor.spiral_position())
            .max()
            .unwrap()
    }

    fn player_count(&self) -> usize {
        self.players().len()
    }
}

pub struct Simulation {
    players: Vec<Player>,
    grid: Option<Grid<PlayerId>>,
    forbiddances: SlidingWindow<PlayerIdSet>,
    simulated_turns: usize,
}

pub struct FinalizedSimulation {
    players: Vec<Player>,
    grid: Arc<FrozenGrid<PlayerId>>,
    simulated_turns: usize,
}

impl FinalizedSimulation {
    pub fn memory_usage(&self) -> MemSize {
        self.grid.memory_usage()
    }

    pub fn empty_cell() -> PlayerId {
        PlayerId::new(0)
    }

    pub fn grid(&self) -> Arc<FrozenGrid<PlayerId>> {
        Arc::clone(&self.grid)
    }

    pub fn highest_player_id(&self) -> PlayerId {
        self.players.iter().map(|p| p.id).max().unwrap_or_default()
    }

    pub fn chunk_count(&self) -> usize {
        self.grid.chunk_count()
    }
}

impl Game for FinalizedSimulation {
    fn players(&self) -> &Vec<Player> {
        &self.players
    }

    fn complete_turns(&self) -> usize {
        self.simulated_turns
    }
}

#[derive(Debug, Eq, PartialEq, Default, Copy, Clone)]
pub struct SimulationProgress {
    memory_usage: MemSize,
    turns: usize,
    complete_shells: usize,
}

impl SimulationProgress {
    pub fn memory_usage(&self) -> MemSize {
        self.memory_usage
    }

    pub fn turns(&self) -> usize {
        self.turns
    }

    pub fn complete_shells(&self) -> usize {
        self.complete_shells
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum SimulationError {
    IsFinalized,
    InfiniteSimulation,
}

#[derive(Debug, PartialEq)]
pub enum SimulationLimit {
    Memory,
    Turns,
    CompleteShells,
    StopFlag,
}

#[derive(Clone)]
pub struct SimulationLimits {
    memory: Option<MemSize>,
    turns: Option<usize>,
    complete_shells: Option<usize>,
    stop_flag: Option<Arc<AtomicBool>>,
}

// NOTE: Only the turn limit is accurate. Other limits are checked on best effort basis as they
//       don't control the simulation directly and may be expensive to check.
impl SimulationLimits {
    pub fn new() -> Self {
        SimulationLimits {
            memory: None,
            turns: None,
            complete_shells: None,
            stop_flag: None,
        }
    }

    pub fn with_memory_limit(mut self, memory: MemSize) -> Self {
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

    pub fn with_stop_flag_limit(mut self, stop_flag: Arc<AtomicBool>) -> Self {
        self.stop_flag = Some(stop_flag);
        self
    }

    pub fn any(&self) -> bool {
        self.memory.is_some() || self.turns.is_some() || self.complete_shells.is_some()
    }

    pub fn memory(&self) -> Option<MemSize> {
        self.memory
    }

    pub fn turns(&self) -> Option<usize> {
        self.turns
    }

    pub fn complete_shells(&self) -> Option<usize> {
        self.complete_shells
    }

    pub fn stop_flag(&self) -> &Option<Arc<AtomicBool>> {
        &self.stop_flag
    }
}

impl Default for SimulationLimits {
    fn default() -> Self {
        Self::new()
    }
}

impl Game for Simulation {
    fn players(&self) -> &Vec<Player> {
        &self.players
    }

    fn complete_turns(&self) -> usize {
        self.simulated_turns
    }
}

impl Simulation {
    pub fn new() -> Simulation {
        Simulation {
            players: vec![],
            grid: Some(Grid::new(
                Box::new(SquareChunker::new(DEFAULT_CHUNK_SIZE)),
                ZstdCompression::new_with_level(6).into(),
            )),
            forbiddances: SlidingWindow::with_origin(0),

            simulated_turns: 0,
        }
    }

    pub fn empty_cell() -> PlayerId {
        PlayerId::new(0)
    }

    pub fn memory_usage(&self) -> MemSize {
        let grid_usage = match self.grid {
            Some(ref grid) => grid.memory_usage(),
            None => MemSize::ZERO,
        };
        grid_usage + self.forbiddances.memory_usage()
    }

    pub fn add_player(&mut self, attacks: LeaperAttacks) -> PlayerId {
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
        if self.simulated_turns > 0 {
            panic!("Cannot modify player enemies in a running simulation.");
        }

        if player.0 as usize > self.players.len() {
            panic!("Player is out of bounds");
        }

        if enemy.0 as usize > self.players.len() {
            panic!("Enemy player is out of bounds");
        }

        self.players[(player.0 - 1) as usize].enemies |= enemy;
    }

    pub fn add_all_pairwise_player_enemies(&mut self) {
        if self.simulated_turns > 0 {
            panic!("Cannot modify player enemies in a running simulation.");
        }

        for player in &mut self.players {
            player.enemies = PlayerIdSet::full() ^ player.id;
        }
    }

    fn grid_region_past_modification(&self) -> Option<GridRect> {
        let last_fully_simulated_shell = match self.complete_shells() {
            0 => return None,
            s => s as i32 - 1,
        };

        let min_point = GridPoint::new(-last_fully_simulated_shell, -last_fully_simulated_shell);

        Some(GridRect::square_with_size(
            min_point,
            2 * last_fully_simulated_shell + 1,
        ))
    }

    #[inline(always)]
    fn simulate_single_turn(&mut self, placements: &mut [Vec<GridPoint>]) {
        for player in self.players.iter_mut() {
            // In a lot of cases we can place the piece immediately where we currently are,
            // so special case it. Results in a significant speedup.
            let immediate_cell = &mut self.forbiddances[player.cursor.spiral_position().as_u64() as isize];
            if !immediate_cell.is_set(player.id) {
                *immediate_cell = PlayerIdSet::full();
            } else {
                // Skip the current element because we checked for it earlier.
                let pos = self
                    .forbiddances
                    .position_or_first_empty(player.cursor.spiral_position().as_u64() as isize + 1.., |x| {
                        !x.is_set(player.id)
                    });
                player.cursor.advance_to(UlamSpiralPoint::new(pos as u64));
                self.forbiddances[player.cursor.spiral_position().as_u64() as isize] = PlayerIdSet::full();
            }

            let attack_src = player.cursor.grid_position();
            placements[player.id.0 as usize].push(attack_src);
            for attack_dst in player.attacks.get_attacks_from(&attack_src) {
                let u = UlamSpiralPoint::from(&attack_dst);
                // We don't care about cells before the origin (last player) and
                // we need to be careful not to modify them.
                if u.as_u64() as isize >= self.forbiddances.get_origin() {
                    self.forbiddances[u.as_u64() as isize] |= player.enemies;
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
            .min_by_key(|player| player.cursor.spiral_position().as_u64())
            .unwrap();
        let new_origin = last_player.cursor.spiral_position().as_u64() as isize;
        self.forbiddances.set_origin(new_origin);
    }

    pub fn simulate(
        &mut self,
        limits: SimulationLimits,
    ) -> Result<SimulationLimit, SimulationError> {
        self.simulate_with_callback(limits, |_| {})
    }

    pub fn simulate_with_callback<F: Fn(SimulationProgress) + Sized>(
        &mut self,
        limits: SimulationLimits,
        progress_callback: F,
    ) -> Result<SimulationLimit, SimulationError> {
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

        if limits.turns.is_some_and(|v| v <= self.simulated_turns) {
            return Ok(SimulationLimit::Turns);
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

        const TURNS_PER_STEP: usize = 1024 * 16;
        // Limit this to have some other cell based processing.
        const MAX_CELLS_PER_STEP: usize = 1024 * 256;
        // We don't want to check for `MAX_STEP_CELLS` being reached every turn
        // because it's not free. This should be safe aside from most extreme cases.
        const TURNS_PER_STEP_MINIBATCH: usize = MAX_CELLS_PER_STEP / 256;
        // We use cells instead of steps as the unit here because the number of cells
        // created in a single step can very wildly, depending on the simulation parameters.
        const COMPRESSION_INTERVAL_CELLS: usize = 1024 * 1024;
        const MEMORY_USAGE_INTERVAL_CELLS: usize = 1024 * 1024;
        // We attempt to amortize chunk freezing so we shouldn't need very many placement buffers.
        const NUM_PLACEMENT_BUFFERS: usize = 8;

        // We transfer ownership of the grid to the worker thread for the time of processing.
        let mut grid = self.grid.take().unwrap();
        let average_cell_count_per_chunk = grid.chunker().average_cell_count();
        let maximum_cells_created_between_compressions = grid
            .chunker()
            .maximum_cells_created_by_spiral_steps(COMPRESSION_INTERVAL_CELLS);
        // We amortize compression, but we can still benefit from some available concurrency in the system.
        let minimum_compression_batch = rayon::current_num_threads() / 2;
        let player_ids = self
            .players
            .iter()
            .map(|player| player.id)
            .collect::<Vec<_>>();
        let (job_tx, job_rx) = mpsc::channel();
        let (buffer_tx, buffer_rx) = mpsc::channel();

        enum Job {
            Place(Vec<Vec<GridPoint>>),
            Compress(GridRect, usize),
            MemoryUsage,
            Stop,
        }

        // Preallocate buffers.
        for _ in 0..NUM_PLACEMENT_BUFFERS {
            let placements: Vec<Vec<GridPoint>> = (0..self.players.len() + 1)
                .map(|i| match i {
                    0 => Vec::new(),
                    _ => Vec::with_capacity(TURNS_PER_STEP),
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

        let grid_memory_usage = Arc::new(Mutex::new(MemSize::ZERO));
        let grid_memory_usage_worker = Arc::clone(&grid_memory_usage);
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
                    Job::Compress(region, n) => {
                        grid.freeze_n(&region, n);
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

        let mut progress = SimulationProgress::default();
        let mut turns_to_simulate = limits.turns.unwrap_or(usize::MAX) - self.simulated_turns;
        let mut hit_limit = None;
        let mut last_compression_farthest_cell = UlamSpiralPoint::new(0);
        let mut last_memory_usage_farthest_cell = UlamSpiralPoint::new(0);
        while turns_to_simulate > 0 {
            let turns_to_simulate_this_step = min(TURNS_PER_STEP, turns_to_simulate);
            let farthest_cell = self.farthest_player_spiral_position();

            let mut placements = get_clear_buffer();
            for bt in (0..turns_to_simulate_this_step).step_by(TURNS_PER_STEP_MINIBATCH) {
                let turns_to_simulate_this_minibatch =
                    min(TURNS_PER_STEP_MINIBATCH, turns_to_simulate_this_step - bt);
                for _ in 0..turns_to_simulate_this_minibatch {
                    // Collect all grid placements first, then we can set them all more efficiently at the end of the step.
                    self.simulate_single_turn(placements.as_mut_slice());
                }
                turns_to_simulate -= turns_to_simulate_this_minibatch;

                // If we advanced by an abnormally high amount of cells then break early
                // to allow other important checks to be made.
                let now_farthest_cell = self.farthest_player_spiral_position();
                if (now_farthest_cell - farthest_cell) as usize > MAX_CELLS_PER_STEP {
                    break;
                }
            }
            job_tx.send(Job::Place(placements)).unwrap();

            // Update the farthest cell because it may have changed significantly during
            // the simulation step.
            let farthest_cell = self.farthest_player_spiral_position();

            // Compress the grid every few steps to reduce memory usage.
            // We don't want to be doing it too often because it requires a whole chunk to be
            // outside the active area and the chunks are large; reduces redundant searches
            // for no longer active chunks.
            //
            // Normally, due to the behavior of the grid, it would lead to exceedingly
            // longer pause times as the simulation progresses if we always froze all
            // eligible chunks. We amortize this behavior by estimating how many chunks
            // we need to freeze every `COMPRESSION_INTERVAL_CELLS` to eventually catch up
            // with the chunks being newly created.
            // When close to the memory limit this amortization scheme is overriden.
            if farthest_cell >= last_compression_farthest_cell + COMPRESSION_INTERVAL_CELLS as u64
                && let Some(region) = self.grid_region_past_modification()
            {
                let new_cells = farthest_cell - last_compression_farthest_cell;
                let n = if limits.memory.is_some_and(|limit| {
                    let total = *grid_memory_usage.lock().unwrap() + self.memory_usage();
                    // Do some best-effort estimation on how much memory can be allocated
                    // before the next check. Early on this will severely overestimate
                    // due to small number of chunks in a straight line being traversed,
                    // but at the same time it shouldn't matter in those cases because
                    // overall memory usage is low at the beginning.
                    // Ideally we would use the chunker here with `new_cell` but the `grid`
                    // is not available at this point.
                    let estimated_new_memory_before_next_check =
                        MemSize::sizes_of::<PlayerId>(maximum_cells_created_between_compressions);
                    total + estimated_new_memory_before_next_check >= limit
                }) {
                    // No limit, force all eligible chunks to be frozen.
                    usize::MAX
                } else {
                    // factor of 2 to overshoot a little
                    max(
                        minimum_compression_batch,
                        (new_cells as usize / average_cell_count_per_chunk + 1) * 2,
                    )
                };

                job_tx.send(Job::Compress(region, n)).unwrap();

                last_compression_farthest_cell = farthest_cell;
            }

            if farthest_cell >= last_memory_usage_farthest_cell + MEMORY_USAGE_INTERVAL_CELLS as u64 {
                // Yes, the check lags behind.
                let total = *grid_memory_usage.lock().unwrap() + self.memory_usage();
                progress.memory_usage = total;

                if limits.memory.is_some_and(|limit| total >= limit) {
                    hit_limit = Some(SimulationLimit::Memory);
                }

                job_tx.send(Job::MemoryUsage).unwrap();

                last_memory_usage_farthest_cell = farthest_cell;
            }

            if limits
                .complete_shells
                .is_some_and(|v| progress.complete_shells >= v)
            {
                hit_limit = Some(SimulationLimit::CompleteShells);
            }

            if limits
                .stop_flag
                .as_ref()
                .is_some_and(|v| v.load(Ordering::Relaxed))
            {
                hit_limit = Some(SimulationLimit::StopFlag);
            }

            progress.complete_shells = self.complete_shells() as usize;
            progress.turns = self.simulated_turns;

            progress_callback(progress);

            if hit_limit.is_some() {
                break;
            }

            self.update_forbiddances_origin();
        }

        // Send final message to terminate the worker thread and get the grid back.
        job_tx.send(Job::Stop).unwrap();
        self.grid = Some(grid_worker.join().unwrap());

        Ok(hit_limit.unwrap_or(SimulationLimit::Turns))
    }

    // Freezes all chunks, deallocates simulation buffers, prohibits further simulation.
    pub fn finalize(self) -> FinalizedSimulation {
        FinalizedSimulation {
            players: self.players,
            grid: Arc::new(self.grid.unwrap().into()),
            simulated_turns: self.simulated_turns,
        }
    }
}

impl Default for Simulation {
    fn default() -> Self {
        Self::new()
    }
}

pub const ULS_MAX_PLAYERS: usize = 63;
pub const ULS_MAX_PLAYER_ID: usize = 64;

impl WriteTo for PlayerId {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        if self.0 as usize > ULS_MAX_PLAYER_ID {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                format!("Player ID {} is too high.", self.0),
            ));
        }
        self.0.write_to(writer)
    }
}

impl ReadFrom for PlayerId {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let id = u8::read_from(reader)?;
        if id as usize > ULS_MAX_PLAYER_ID {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                format!("Player ID {} is too high.", id),
            ));
        }
        Ok(PlayerId(id))
    }
}

impl WriteTo for PlayerIdSet {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.bits.write_to(writer)
    }
}

impl ReadFrom for PlayerIdSet {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let bits = u64::read_from(reader)?;
        if (bits & 1) == 1 {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                "PlayerIdSet must not have the lowest bit set.",
            ));
        }

        Ok(PlayerIdSet { bits })
    }
}

impl WriteTo for Player {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.attacks.write_to(writer)?;
        self.id.write_to(writer)?;
        self.enemies.write_to(writer)?;
        self.cursor.write_to(writer)?;
        Ok(())
    }
}

impl ReadFrom for Player {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        Ok(Self {
            attacks: LeaperAttacks::read_from(reader)?,
            id: PlayerId::read_from(reader)?,
            enemies: PlayerIdSet::read_from(reader)?,
            cursor: UlamSpiralCursor::read_from(reader)?,
        })
    }
}

impl WriteTo for FinalizedSimulation {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.players.write_to(writer)?;
        self.simulated_turns.write_to(writer)?;
        // When all chunks are finalized we can compress them even more as a sequence.
        let mut encoder = zstd::Encoder::new(writer, 3)?;
        self.grid.write_to(&mut encoder)?;
        encoder.flush()?;
        Ok(())
    }
}

impl ReadFrom for FinalizedSimulation {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let players = Vec::<Player>::read_from(reader)?;
        for (i, player) in players.iter().enumerate() {
            if player.id.0 as usize != i + 1 {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidData,
                    "Player ID must match its position.",
                ));
            }

            if player.enemies.highest_player_id().0 as usize > players.len() {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidData,
                    "Player is enemy with non-existent player.",
                ));
            }
        }

        let simulated_turns = usize::read_from(reader)?;
        let grid = FrozenGrid::<PlayerId>::read_from(&mut zstd::Decoder::new(reader)?)?;

        Ok(FinalizedSimulation {
            players,
            simulated_turns,
            grid: Arc::new(grid),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::piece::LeaperAttacks;
    use crate::math::coords::GridVector;

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
        assert_eq!(sim.complete_turns(), 100);
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
        assert_eq!(grid[GridPoint::new(-1, 0)], p1);
        assert_eq!(grid[GridPoint::new(2, -1)], p1);
        assert_eq!(grid[GridPoint::new(2, 1)], p1);

        assert_eq!(grid[GridPoint::new(-1, 1)], p2);
        assert_eq!(grid[GridPoint::new(-1, -1)], p2);
        assert_eq!(grid[GridPoint::new(2, 0)], p2);
    }
}
