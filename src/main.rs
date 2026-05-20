use ulam_leapers::grid::GridVector;
use ulam_leapers::piece::LeaperAttacks;
use ulam_leapers::simulation::Simulation;

fn main() {
    let mut sim = Simulation::new(10);
    let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
    let p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
    sim.add_player_threat(p1, p2);
    sim.add_player_threat(p2, p1);
}
