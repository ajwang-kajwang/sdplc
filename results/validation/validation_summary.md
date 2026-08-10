# SD-PLC Sprint 4 Validation Summary

## Scope

This run produced repeatable evidence for compiler compilation, deterministic runtime scan timing, flotation-tank telemetry, and wire-level OPC UA read/write latency.

Runtime cycle count: `1000`.

## Artefacts

| Artefact | Purpose |
|---|---|
| `scan_timing_10ms.csv` | Flotation runtime scan timing at 10 ms (present) |
| `scan_timing_20ms.csv` | Flotation runtime scan timing at 20 ms (present) |
| `scan_timing_50ms.csv` | Flotation runtime scan timing at 50 ms (present) |
| `compiler_pipeline_benchmark.csv` | Compiler pipeline phase timings (present) |
| `control_flow_scan_timing_10ms.csv` | Control-flow ST runtime scan timing at 10 ms (present) |
| `flotation_tank_telemetry.csv` | Deterministic flotation-tank telemetry trend (present) |
| `opcua_read_latency.csv` | OPC UA read response latency samples (present) |
| `opcua_write_latency.csv` | OPC UA write response latency samples (present) |
| `validation_summary.md` | Thesis-ready summary of the validation run (present) |

## Commands

| Step | Result | Wall time (s) | Command |
|---|---|---:|---|
| compiler flotation | pass | 0.488 | `cargo run --quiet --bin sdplc -- examples/flotation_tank.st -o results\flotation_tank -q` |
| compiler control_flow | pass | 0.466 | `cargo run --quiet --bin sdplc -- programs/control_flow.st -o results\control_flow -q` |
| flotation runtime 10ms | pass | 11.036 | `cargo run --quiet --bin runtime -- examples/flotation_tank.st --cycles=1000 --scan-time=10 --out=results\.scan-timing-10ms -q` |
| flotation runtime 20ms | pass | 21.477 | `cargo run --quiet --bin runtime -- examples/flotation_tank.st --cycles=1000 --scan-time=20 --out=results\.scan-timing-20ms -q` |
| flotation runtime 50ms | pass | 51.589 | `cargo run --quiet --bin runtime -- examples/flotation_tank.st --cycles=1000 --scan-time=50 --out=results\.scan-timing-50ms -q` |
| control_flow runtime 10ms | pass | 11.536 | `cargo run --quiet --bin runtime -- programs/control_flow.st --cycles=1000 --scan-time=10 --out=results\.control-flow-scan-timing-10ms -q` |
| opcua read/write latency | pass | 2.153 | `cargo run --quiet --bin opcua_server -- examples/flotation_tank.st --scan-time=10 --self-test --read-count=1000 --write-count=100 --out=results` |

## Thesis Claim Boundary

Evidence in this folder supports claims for a Rust ST compiler frontend, representative semantic/code generation support, a deterministic scan-cycle runtime prototype, a flotation-tank validation case study, and OPC UA read/write exposure of process variables. It does not claim full O-PAS compliance.

