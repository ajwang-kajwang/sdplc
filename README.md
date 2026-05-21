# SD-PLC

A Rust IEC 61131-3 Structured Text compiler and runtime validation prototype targeting LLVM IR.

## Overview

SD-PLC compiles Structured Text through a conventional compiler pipeline:

```text
Structured Text source
  -> lexer
  -> parser / AST
  -> semantic analyser
  -> LLVM IR code generation
  -> deterministic runtime / validation harness
```

The current thesis-focused branch narrows the project toward vendor-agnostic ST compilation, scan-cycle runtime evidence, flotation-tank simulation telemetry, and OPC UA address-space mapping.

## Current Status

| Area | Status |
|------|--------|
| Lexer | Complete: ST tokens, comments, literals, control-flow keywords |
| Parser / AST | Complete: POUs, variables, expressions, arrays, control flow |
| Semantic analysis | Complete: type resolution, scope validation, diagnostics |
| LLVM IR generation | Complete: representative ST constructs via inkwell |
| Runtime | Sprint 4: JIT scan-cycle executor refreshes a typed process image and exports benchmark CSVs |
| Process image | Available: typed deterministic process variable store |
| Validation harness | Sprint 4: repeatable compiler/runtime/OPC UA validation pack |
| OPC UA | Sprint 4: pure Rust OPC UA server exposes tank/runtime variables and read/write latency evidence |

## Build And Test

```bash
cargo fmt
cargo test
```

Sprint validation commands:

```bash
cargo run --bin validate_sim -- --cycles=1000 --scan-time=10
cargo run --bin sdplc -- programs/control_flow.st -o results/control_flow
cargo run --bin runtime -- programs/control_flow.st --cycles=100 --scan-time=10 -q
cargo run --bin runtime -- examples/flotation_tank.st --cycles=1000 --scan-time=10 -q
cargo run --bin opcua_server -- examples/flotation_tank.st --scan-time=10 --self-test
cargo run --bin demo_validation -- --cycles=1000
```

The validation commands write evidence into the `results/` folder. The current
evidence layout is:

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

## Running The Compiler

Compile a Structured Text file:

```bash
cargo run --bin sdplc -- programs/control_flow.st
```

Write a custom output name:

```bash
cargo run --bin sdplc -- programs/control_flow.st -o results/control_flow
```

Print LLVM IR to stdout:

```bash
cargo run --bin sdplc -- programs/control_flow.st --emit-ir
```

Run the built-in demo:

```bash
cargo run --bin sdplc
```

## Runtime

The runtime JIT-compiles an ST program and executes it in a scan-cycle loop:

```bash
cargo run --bin runtime -- programs/control_flow.st --scan-time=50
cargo run --bin runtime -- programs/control_flow.st --cycles=500
cargo run --bin runtime -- programs/control_flow.st --cycles=100 -q
cargo run --bin runtime -- examples/flotation_tank.st --cycles=1000 --scan-time=10 -q
```

In interactive mode, the dashboard shows PROGRAM variables from the runtime `ProcessImage`, scan timing, and final values. Quiet mode prints the summary only, which is useful for repeatable validation runs. A default runtime run writes `runtime_scan_timing.csv` and `runtime_final_values.csv` inside the selected output directory. The current thesis evidence keeps the latest runtime files under `results/runtime/latest/`, with scan-period-specific evidence under `results/runtime/flotation_tank/`.

## Validation Harness

`validate_sim` runs the deterministic flotation-tank model without requiring hardware. It records scan timing metrics, telemetry, and the OPC UA node mapping scaffold:

```bash
cargo run --bin validate_sim -- --cycles=1000 --scan-time=10
```

Useful options:

```bash
--cycles=N       Number of scan cycles to execute, default 1000
--scan-time=MS   Scan period in milliseconds, default 10
--out=DIR        Output directory, default results
```

## Sprint 4 Validation Pack

Run the full thesis validation pack:

```bash
cargo run --bin demo_validation -- --cycles=1000
```

For a short smoke run while developing:

```bash
cargo run --bin demo_validation -- --quick --out=results_sprint4_quick
```

The full runner compiles the flotation and control-flow ST programs, runs the runtime scan-cycle matrix, records flotation telemetry, starts the OPC UA server self-test client, writes read/write latency CSVs, and creates `results/validation/validation_summary.md`.

## OPC UA Server

Sprint 3 adds a pure Rust OPC UA server backend using `async-opcua-server`. It exposes the flotation-tank process image and runtime metrics under `Objects/SDPLC`:

```bash
cargo run --bin opcua_server -- examples/flotation_tank.st --scan-time=10
```

Useful options:

```bash
--host=ADDR       Bind host, default 127.0.0.1
--port=PORT       Bind port, default 4855
--scan-time=MS    Simulation scan period, default 10
--duration=SEC    Stop automatically after SEC seconds
--self-test       Run a local OPC UA browse/read/write smoke client
--out=DIR         Output directory, default results
```

The endpoint is `opc.tcp://127.0.0.1:4855/` by default. The server writes address-space evidence, read-value evidence, and test notes into the selected output directory. The current thesis evidence keeps these files under `results/opcua/address_space/`. With `--self-test`, it also writes client smoke-test evidence and latency CSVs; the current thesis files are under `results/opcua/self_test/` and `results/opcua/latency/`. Writable tank nodes use OPC UA write callbacks that update the shared `ProcessImage`; runtime metrics and calculated grade remain read-only to clients.

Sprint 4 latency options:

```bash
--read-count=N    OPC UA self-test read latency samples, default 1000
--write-count=N   OPC UA self-test write latency samples, default 100
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
|   `-- Program/
|       |-- Floatation Tank.project
|       |-- Floatation Tank-AllUsers.opt
|       |-- Floatation Tank-kajwa-RYUTEI.opt
|       |-- Floatation Tank_project.precompilecache
|       |-- Floatation Tank.~u
|       |-- *.simulation.bootinfo
|       |-- *.simulation.bootinfo_guids
|       `-- *.simulation.compileinfo
|-- docs/
|   |-- Developer Guide.md
|   `-- sprint_roadmap.md
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
`-- thesis_figures/
    `-- chapter5/
        |-- generate_chapter5_figures.py
        `-- output/
            |-- PLACEMENT_MAP.md
            |-- figure_5_3_startup_level_codesys_vs_sdplc.png
            |-- figure_5_4_airflow_grade_codesys_vs_sdplc.png
            |-- figure_5_6_opcua_read_latency_histogram.png
            `-- figure_5_7_opcua_write_latency_histogram.png
```

## Compiling To Native Code

After the compiler generates `.ll` and `.bc` files, use LLVM tools to target a native platform:

```bash
llc results/control_flow.ll -o results/control_flow.s
gcc results/control_flow.s -o results/control_flow
```

Cross-compilation is handled by LLVM target triples, for example:

```bash
llc results/control_flow.ll -mtriple=aarch64-linux-gnu -o results/control_flow_arm64.s
llc results/control_flow.ll -mtriple=armv5te-linux-gnueabi -o results/control_flow_armv5.s
```

## Roadmap

The active completion plan is in [docs/sprint_roadmap.md](docs/sprint_roadmap.md). Sprint 4 adds the repeatable validation pack, compiler/runtime benchmark artefacts, flotation telemetry, OPC UA latency CSVs, and a thesis-ready validation summary.
