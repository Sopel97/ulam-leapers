use crate::gui::app::App;
use eframe::wgpu::PresentMode;
use std::io::Read;
use ulam_leapers::game::piece::LeaperAttacks;
use ulam_leapers::game::simulation::{FinalizedSimulation, Game, Simulation, SimulationLimits};
use ulam_leapers::io::{ReadFrom, WriteTo};
use ulam_leapers::math::coords::GridVector;
use ulam_leapers::util::memory::MemSize;

const GUI: bool = true;

pub mod gui;

fn main() {
    if GUI {
        let mut options = eframe::NativeOptions::default();
        options.wgpu_options.present_mode = PresentMode::AutoVsync;
        options.wgpu_options.desired_maximum_frame_latency = Some(1);
        options.vsync = true;

        let result = eframe::run_native(
            "Ulam Leapers Explorer",
            options,
            Box::new(|cc| Ok(Box::new(App::new(cc)))),
        );

        match result {
            Ok(()) => {}
            Err(err) => {
                eprintln!("Error: {err}");
            }
        }
    } else {
        let mut sim = Simulation::new();
        let p1 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        let p2 = sim.add_player(LeaperAttacks::from_canonical(&GridVector::new(1, 2)));
        sim.add_player_enemy(p1, p2);
        sim.add_player_enemy(p2, p1);

        let start = std::time::Instant::now();
        let _ = sim.simulate(
            SimulationLimits::new()
                .with_turn_limit(100_000_000)
                .with_memory_limit(MemSize::gb(32)),
        );
        let end_memory_usage = sim.memory_usage();
        let finalized_sim = sim.finalize();
        let elapsed = start.elapsed();

        let simulated_turns = finalized_sim.complete_turns();
        let complete_shells = finalized_sim.complete_shells();
        let finalized_memory_usage = finalized_sim.memory_usage();
        println!(
            "Simulated {} turns in {:?}.\nComplete shells: {}.\nEstimated memory usage: {}.\nFinal memory usage: {}.\nChunk count: {}",
            simulated_turns,
            elapsed,
            complete_shells,
            end_memory_usage.display().si(),
            finalized_memory_usage.display().si(),
            finalized_sim.chunk_count(),
        );

        let mut serialized = Vec::<u8>::with_capacity(1024);
        finalized_sim.write_to(&mut serialized).unwrap();
        println!("{}", serialized.len());
        println!("{:?}", serialized[..128].bytes());

        let deserialized = FinalizedSimulation::read_from(&mut serialized.as_slice()).unwrap();
        println!(
            "{} {}",
            deserialized.memory_usage().display().si(),
            deserialized.chunk_count()
        );
    }
}
