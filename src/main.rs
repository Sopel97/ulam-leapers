use ulam_leapers::grid::GridVector;
use ulam_leapers::piece::LeaperAttacks;
use ulam_leapers::simulation::Simulation;

fn main() {
    let mut sim = Simulation::new();
    sim.set_max_memory_usage(100_000_000);
    let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
    let p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
    sim.add_player_enemy(p1, p2);
    sim.add_player_enemy(p2, p1);

    let start = std::time::Instant::now();
    let res = sim.simulate(100_000_000);
    let elapsed = start.elapsed();

    println!("Simulated {} turns in {:?} with {} MiB of memory.", sim.simulated_turns(), elapsed, sim.memory_usage() / 1024 / 1024);
}
