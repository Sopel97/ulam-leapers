use ulam_leapers::grid::GridVector;
use ulam_leapers::piece::LeaperAttacks;
use ulam_leapers::simulation::{Simulation, SimulationLimits};

fn main() {
    let mut sim = Simulation::new();
    let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
    let p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
    sim.add_player_enemy(p1, p2);
    sim.add_player_enemy(p2, p1);

    let start = std::time::Instant::now();
    let _ = sim.simulate(SimulationLimits::new().with_turn_limit(100_000_000).with_memory_limit(32*1024*1024*1024));
    let end_memory_usage = sim.memory_usage();
    sim.finalize();
    let elapsed = start.elapsed();

    let finalized_memory_usage = sim.memory_usage();
    println!(
        "Simulated {} turns in {:?}.\nComplete shells: {}.\nEstimated memory usage: {} MiB.\nFinal memory usage: {} MiB.",
        sim.simulated_turns(),
        elapsed,
        sim.complete_shells(),
        end_memory_usage / 1024 / 1024,
        finalized_memory_usage / 1024 / 1024
    );
}
