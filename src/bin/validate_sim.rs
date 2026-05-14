//! Validation runner for the flotation-tank sprint.
//!
//! This binary produces two thesis-useful artefacts without requiring
//! physical hardware:
//! - scan timing metrics
//! - flotation-tank telemetry
//!
//! Usage:
//!   cargo run --bin validate_sim -- --cycles=1000 --scan-time=10

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use sdplc::opcua_bridge::OpcUaAddressSpace;
use sdplc::simulation::FlotationTankSim;
use sdplc::timing::ScanTiming;

#[derive(Debug, Clone)]
struct Config {
    cycles: u64,
    scan_time_ms: u64,
    output_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cycles: 1000,
            scan_time_ms: 10,
            output_dir: PathBuf::from("results"),
        }
    }
}

fn main() {
    let config = parse_args();
    fs::create_dir_all(&config.output_dir).expect("failed to create results directory");

    let scan_duration = Duration::from_millis(config.scan_time_ms);
    let mut timing = ScanTiming::new(scan_duration);
    let mut sim = FlotationTankSim::default();
    let image = sim.seed_process_image();
    let address_space = OpcUaAddressSpace::from_process_image("urn:sdplc:validation", &image);

    let mut telemetry = Vec::new();
    telemetry.push(FlotationTankSim::telemetry_csv_header().to_string());

    let mut opcua_nodes = Vec::new();
    opcua_nodes.push(OpcUaAddressSpace::csv_header().to_string());
    opcua_nodes.extend(address_space.csv_rows());

    for cycle in 0..config.cycles {
        let cycle_start = Instant::now();

        // Placeholder for Sprint 2 compiled ST execution hook:
        // compiled_scan(&mut process_image);
        // For now this validates the deterministic plant model and timing path.
        if cycle == config.cycles / 3 {
            sim.air_flow = 55.0;
        }
        if cycle == (config.cycles * 2) / 3 {
            sim.tailings_flow = 43.0;
        }

        sim.step(scan_duration.as_secs_f64());
        let exec_time = cycle_start.elapsed();

        let elapsed = cycle_start.elapsed();
        if elapsed < scan_duration {
            std::thread::sleep(scan_duration - elapsed);
        }
        let total_cycle_time = cycle_start.elapsed();
        timing.record_cycle(exec_time, total_cycle_time);

        telemetry.push(sim.telemetry_csv_row(cycle));
    }

    let timing_path = config.output_dir.join("scan_timing.csv");
    let telemetry_path = config.output_dir.join("flotation_tank_telemetry.csv");
    let opcua_path = config.output_dir.join("opcua_address_space.csv");

    fs::write(
        &timing_path,
        format!("{}\n{}\n", ScanTiming::csv_header(), timing.csv_row()),
    )
    .expect("failed to write scan timing csv");
    fs::write(&telemetry_path, telemetry.join("\n") + "\n").expect("failed to write telemetry csv");
    fs::write(&opcua_path, opcua_nodes.join("\n") + "\n")
        .expect("failed to write opc ua address space csv");

    println!("SD-PLC validation run complete");
    println!("  cycles:        {}", timing.cycles());
    println!("  scan target:   {} ms", config.scan_time_ms);
    println!("  avg exec:      {:.3} us", timing.avg_exec_us());
    println!("  max exec:      {:.3} us", timing.max_exec_us());
    println!("  avg jitter:    {:.3} us", timing.avg_jitter_us());
    println!("  max jitter:    {:.3} us", timing.max_jitter_us());
    println!("  OPC UA nodes:  {}", address_space.nodes().len());
    println!("  wrote:         {}", timing_path.display());
    println!("  wrote:         {}", telemetry_path.display());
    println!("  wrote:         {}", opcua_path.display());
}

fn parse_args() -> Config {
    let mut config = Config::default();

    for arg in env::args().skip(1) {
        if let Some(value) = arg.strip_prefix("--cycles=") {
            config.cycles = value.parse().expect("--cycles must be a positive integer");
        } else if let Some(value) = arg.strip_prefix("--scan-time=") {
            config.scan_time_ms = value
                .parse()
                .expect("--scan-time must be a positive integer");
        } else if let Some(value) = arg.strip_prefix("--out=") {
            config.output_dir = PathBuf::from(value);
        } else if arg == "--help" || arg == "-h" {
            print_help_and_exit();
        } else {
            eprintln!("unknown argument: {}", arg);
            print_help_and_exit();
        }
    }

    config
}

fn print_help_and_exit() -> ! {
    println!("SD-PLC flotation validation runner\n");
    println!("USAGE:");
    println!("  cargo run --bin validate_sim -- --cycles=1000 --scan-time=10");
    println!();
    println!("OPTIONS:");
    println!("  --cycles=N       Number of scan cycles to execute, default 1000");
    println!("  --scan-time=MS   Scan period in milliseconds, default 10");
    println!("  --out=DIR        Output directory, default results");
    std::process::exit(0);
}
