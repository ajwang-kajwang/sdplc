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
| Runtime | Available: JIT scan-cycle executor exposed as `runtime` |
| Process image | Available: typed deterministic process variable store |
| Validation harness | Available: flotation simulation and timing CSV output |
| OPC UA | Scaffolded: address-space CSV mapping, server backend pending |

## Build And Test

```bash
cargo fmt
cargo test
```

Sprint 1 validation commands:

```bash
cargo run --bin validate_sim -- --cycles=1000 --scan-time=10
cargo run --bin sdplc -- programs/control_flow.st -o results/control_flow
cargo run --bin runtime -- programs/control_flow.st --cycles=100 --scan-time=10 -q
```

The validation runner writes:

```text
results/scan_timing.csv
results/flotation_tank_telemetry.csv
results/opcua_address_space.csv
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
```

In interactive mode, the dashboard shows PROGRAM variables, scan timing, and final values. Quiet mode prints the summary only, which is useful for repeatable validation runs.

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

## Project Structure

```text
sdplc/
|-- Cargo.toml
|-- README.md
|-- docs/
|   `-- sprint_roadmap.md
|-- programs/
|   `-- control_flow.st
|-- src/
|   |-- main.rs              # compiler CLI (`sdplc`)
|   |-- bin/
|   |   |-- runtime.rs       # JIT scan-cycle runtime
|   |   `-- validate_sim.rs  # flotation validation runner
|   |-- ast.rs
|   |-- codegen.rs
|   |-- lexer.rs
|   |-- parser.rs
|   |-- semantic.rs
|   |-- process_image.rs
|   |-- timing.rs
|   |-- simulation.rs
|   `-- opcua_bridge.rs
`-- tests/
    |-- lexer_integration.rs
    |-- parser_integration.rs
    |-- sementic_integration.rs
    `-- codegen_integration_test.rs
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

The active completion plan is in [docs/sprint_roadmap.md](docs/sprint_roadmap.md). Sprint 1 is focused on build stability, validation CSV generation, runtime binary exposure, and matching repository documentation.
