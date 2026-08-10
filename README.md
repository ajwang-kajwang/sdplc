# SD-PLC

A Rust IEC 61131-3 Structured Text compiler and runtime validation prototype targeting LLVM IR.
The system depends on `LLVM17` exposed via the `inkwell` crate.

## Overview

SD-PLC compiles Structured Text through a standard compiler pipeline:

```text
Structured Text source
  -> lexer
  -> parser / AST
  -> semantic analyser
  -> LLVM IR code generation
  -> deterministic runtime / validation harness
```

## Running The Compiler

Run the built-in demo:

```bash
cargo run --bin sdplc
```

Compile a Structured Text file:

```bash
cargo run --bin sdplc -- programs/flotation_tank.st
```

## Compiling To Native Code

After the compiler generates `.ll` and `.bc` files, use LLVM tools to target a native platform:

Cross-compilation examples:

```bash
llc results/compiler_ir/floatation_tank/flotation_tank.ll -mtriple=aarch64-linux-gnu -o results/control_flow_arm64.s
llc results/compiler_ir/floatation_tank/flotation_tank.ll -mtriple=armv5te-linux-gnueabi -o results/control_flow_armv5.s
```

## Runtime

The runtime JIT-compiles an ST program and executes it in a scan-cycle loop:

```bash
cargo run --bin runtime -- programs/flotation_tank.st --cycles=1000 --scan-time=10
```

## Validation Commands

```bash
cargo run --bin validate_sim -- --cycles=1000 --scan-time=10
cargo run --bin runtime -- programs/flotation_tank.st --cycles=1000 --scan-time=10 -q
cargo run --bin opcua_server -- programs/flotation_tank.st --scan-time=10 --self-test
```

The validation commands write evidence into the `results/` folder.

```text
results/
|-- compiler_benchmark/
|   `-- compiler_pipeline_benchmark.csv
|-- compiler_ir/
|   |-- control_flow/
|   `-- flotation_tank/
|-- runtime/
|   |-- control_flow/
|   |   `-- control_flow_scan_timing_10ms.csv
|   |-- flotation_tank/
|   |   |-- scan_timing_10ms.csv
|   |   |-- scan_timing_20ms.csv
|   |   `-- scan_timing_50ms.csv
|   `-- latest/
|       |-- runtime_final_values.csv
|       `-- runtime_scan_timing.csv
|-- simulation/
|   `-- flotation_tank/
|       |-- flotation_tank_telemetry.csv
|       `-- scan_timing.csv
|-- opcua/
|   |-- address_space/
|   |   |-- opcua_address_space.csv
|   |   |-- opcua_read_values.csv
|   |   `-- opcua_test_notes.md
|   |-- latency/
|   |   |-- opcua_read_latency.csv
|   |   `-- opcua_write_latency.csv
|   `-- self_test/
|       `-- opcua_client_smoke.csv
`-- validation/
    `-- validation_summary.md
```

## Project Structure

```text
sdplc/
|-- .cargo/
|   `-- config.toml             # local Cargo / LLVM build configuration
|-- Cargo.toml
|-- Cargo.lock
|-- README.md
|-- benchmark/
|   |-- convert_trace.py        # converts CODESYS Trace exports into comparison CSVs
|   |-- codesys_flotation_10ms.csv
|   |-- codesys_flotation_10ms_raw.csv
|   |-- codesys_flotation_20ms.csv
|   |-- codesys_flotation_20ms_raw.csv
|   |-- codesys_flotation_50ms.csv
|   |-- codesys_flotation_50ms_raw.csv
|-- examples/
|   `-- flotation_tank.st
|-- programs/
|   `-- control_flow.st
|-- results/
|   |-- README.md
|   |-- compiler_benchmark/
|   |   `-- compiler_pipeline_benchmark.csv
|   |-- compiler_ir/
|   |   |-- control_flow/
|   |   |   |-- control_flow.bc
|   |   |   `-- control_flow.ll
|   |   `-- flotation_tank/
|   |       |-- flotation_tank.bc
|   |       `-- flotation_tank.ll
|   |-- runtime/
|   |   |-- control_flow/
|   |   |   `-- control_flow_scan_timing_10ms.csv
|   |   |-- flotation_tank/
|   |   |   |-- scan_timing_10ms.csv
|   |   |   |-- scan_timing_20ms.csv
|   |   |   `-- scan_timing_50ms.csv
|   |   `-- latest/
|   |       |-- runtime_final_values.csv
|   |       `-- runtime_scan_timing.csv
|   |-- simulation/
|   |   `-- flotation_tank/
|   |       |-- flotation_tank_telemetry.csv
|   |       `-- scan_timing.csv
|   |-- opcua/
|   |   |-- address_space/
|   |   |   |-- opcua_address_space.csv
|   |   |   |-- opcua_read_values.csv
|   |   |   `-- opcua_test_notes.md
|   |   |-- latency/
|   |   |   |-- opcua_read_latency.csv
|   |   |   `-- opcua_write_latency.csv
|   |   `-- self_test/
|   |       `-- opcua_client_smoke.csv
|   `-- validation/
|       `-- validation_summary.md
|-- src/
|   |-- main.rs                 # compiler CLI (`sdplc`)
|   |-- lib.rs                  # shared library entry point
|   |-- bin/
|   |   |-- demo_validation.rs   # thesis validation pack runner
|   |   |-- opcua_server.rs      # OPC UA server and self-test client
|   |   |-- runtime.rs           # JIT scan-cycle runtime
|   |   `-- validate_sim.rs      # flotation simulation validation runner
|   |-- ast.rs                   # Structured Text AST definitions
|   |-- lexer.rs                 # tokenisation
|   |-- parser.rs                # POU, declaration and statement parsing
|   |-- semantic.rs              # type checking and diagnostics
|   |-- codegen.rs               # LLVM IR generation through inkwell
|   |-- process_image.rs         # typed runtime process-image store
|   |-- timing.rs                # scan-cycle timing metrics
|   |-- simulation.rs            # deterministic flotation-tank model
|   `-- opcua_bridge.rs          # process-image to OPC UA node mapping
|-- tests/
|   |-- lexer_integration.rs
|   |-- parser_integration.rs
|   |-- sementic_integration.rs
|   `-- codegen_integration_test.rs
```
