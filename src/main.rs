use ulam_leapers::grid::GridVector;
use ulam_leapers::piece::LeaperAttacks;
use ulam_leapers::simulation::Simulation;

fn main() {
    let mut sim = Simulation::new();
    let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
    let p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
    sim.add_player_enemy(p1, p2);
    sim.add_player_enemy(p2, p1);

    let start = std::time::Instant::now();
    let _ = sim.simulate(100_000_000);
    let end_memory_usage = sim.memory_usage();
    sim.finalize();
    let elapsed = start.elapsed();

    let finalized_memory_usage = sim.memory_usage();
    println!(
        "Simulated {} turns in {:?} with {} MiB of memory -> {} MiB finalized.",
        sim.simulated_turns(),
        elapsed,
        end_memory_usage / 1024 / 1024,
        finalized_memory_usage / 1024 / 1024
    );
}
