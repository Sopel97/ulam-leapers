use std::io::Read;
use ulam_leapers::grid::GridVector;
use ulam_leapers::io::{WriteTo, ReadFrom};
use ulam_leapers::piece::LeaperAttacks;
use ulam_leapers::simulation::{FinalizedSimulation, Game, Simulation, SimulationLimits};

const GUI: bool = true;

mod gui;

fn main() {
    if GUI {
        gui::run().unwrap(); 
    } else {
        let mut sim = Simulation::new();
        let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        let p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        sim.add_player_enemy(p1, p2);
        sim.add_player_enemy(p2, p1);

        let start = std::time::Instant::now();
        let _ = sim.simulate(SimulationLimits::new().with_turn_limit(100_000_000).with_memory_limit(32 * 1024 * 1024 * 1024));
        let end_memory_usage = sim.memory_usage();
        let finalized_sim = sim.finalize();
        let elapsed = start.elapsed();

        let simulated_turns = finalized_sim.simulated_turns();
        let complete_shells = finalized_sim.complete_shells();
        let finalized_memory_usage = finalized_sim.memory_usage();
        println!(
            "Simulated {} turns in {:?}.\nComplete shells: {}.\nEstimated memory usage: {} MiB.\nFinal memory usage: {} bytes.\nChunk count: {}",
            simulated_turns,
            elapsed,
            complete_shells,
            end_memory_usage / 1024 / 1024,
            finalized_memory_usage,
            finalized_sim.chunk_count(),
        );

        let mut serialized = Vec::<u8>::with_capacity(1024);
        finalized_sim.write_to(&mut serialized).unwrap();
        println!("{}", serialized.len());
        println!("{:?}", serialized[..128].bytes());

        let deserialized = FinalizedSimulation::read_from(&mut serialized.as_slice()).unwrap();
        println!("{} {}", deserialized.memory_usage(), deserialized.chunk_count());
    }
}
