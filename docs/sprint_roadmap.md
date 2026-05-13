# SD-PLC One-Week Completion Roadmap

Branch: `sprint/opcua-validation-roadmap`

## Project framing

The project is no longer trying to deliver a web IDE or container orchestration. The revised completion target is:

```text
Structured Text source
  -> existing Rust lexer/parser/semantic analyser
  -> LLVM IR/code generation
  -> deterministic runtime / process image
  -> flotation-tank simulated I/O validation
  -> OPC UA server exposing process variables
  -> thesis-ready benchmark artefacts
```

Containerisation is deliberately dropped. The remaining thesis claim is narrower and stronger: SD-PLC demonstrates a vendor-agnostic IEC 61131-3 Structured Text compilation and execution backend with deterministic scan-cycle validation and OPC UA industrial connectivity.

## Current sprint changes

This branch adds the support modules needed to move from a compiler-only repository to a validation-and-connectivity prototype.

### Added

- `src/process_image.rs`
  - Typed process image for runtime variables.
  - Supports `BOOL`, signed/unsigned integer-like values, floating-point values, and strings.
  - Provides sorted deterministic iteration for validation and OPC UA mapping.

- `src/timing.rs`
  - Scan-cycle timing metrics.
  - Exports CSV rows for thesis results.

- `src/simulation.rs`
  - Deterministic flotation-tank process model.
  - Provides named process variables for level, air flow, feed flow, tailings flow, concentrate grade, emergency stop, and motor running.

- `src/opcua_bridge.rs`
  - OPC UA address-space mapping scaffold.
  - Converts process variables to node specifications such as `ns=2;s=SDPLC.tank.level`.
  - Backend-neutral by design so the next sprint can bind it to `open62541` or a Rust OPC UA server crate.

- `src/bin/validate_sim.rs`
  - Validation runner producing:
    - `results/scan_timing.csv`
    - `results/flotation_tank_telemetry.csv`
    - `results/opcua_address_space.csv`

- `Cargo.toml`
  - Explicitly exposes:
    - `sdplc`
    - `runtime`
    - `validate_sim`

## Immediate commands to run

```bash
cargo fmt
cargo test
cargo run --bin validate_sim -- --cycles=1000 --scan-time=10
cargo run --bin sdplc -- programs/control_flow.st -o results/control_flow
cargo run --bin runtime -- programs/control_flow.st --cycles=100 --scan-time=10 -q
```

If `runtime` fails to compile, first check whether `src/codegen.rs` actually exposes `compile_for_runtime`. `src/runtime.rs` currently expects that symbol. If it is missing, Sprint 2 must either implement it or temporarily modify `runtime` to call the existing `compile` path.

## Sprint plan

### Sprint 1: Stabilise build and validation harness

Target: same day.

Deliverables:

- `cargo test` passes.
- `cargo run --bin validate_sim` writes CSV files.
- Runtime binary is exposed through Cargo.
- README updated so its commands match the repository.

Definition of done:

```bash
cargo fmt
cargo test
cargo run --bin validate_sim -- --cycles=1000 --scan-time=10
```

### Sprint 2: Connect compiled ST to the process image

Target: next working session.

Deliverables:

- Runtime creates a `ProcessImage` from compiled PROGRAM variables.
- Compiled scan execution updates process-image variables after every cycle.
- Flotation Tank ST example can be run through the runtime.
- Runtime can export `results/runtime_scan_timing.csv`.

Implementation notes:

1. Keep the first integration narrow.
2. Start with top-level scalar PROGRAM variables only.
3. Treat arrays/function blocks as internal compiler features, not process-image exports, unless needed by the flotation example.
4. Add variable metadata to `CodeGenerator::compile_for_runtime`:
   - variable name
   - resolved type
   - getter function name
   - optional setter function name
5. Use the existing JIT getter pattern in `src/runtime.rs` as the bridge.

Definition of done:

```bash
cargo run --bin runtime -- examples/flotation_tank.st --cycles=1000 --scan-time=10 -q
```

Expected artefact:

```text
results/runtime_scan_timing.csv
results/runtime_final_values.csv
```

### Sprint 3: OPC UA server backend

Target: mid-week.

Deliverables:

- Choose backend:
  - preferred: `open62541` FFI if system library setup is manageable;
  - fallback: pure Rust OPC UA crate if it can expose variable nodes quickly;
  - final fallback: generated address-space CSV + documented server binding plan, only if build risk becomes too high.
- Create `src/bin/opcua_server.rs`.
- Expose process variables as OPC UA nodes.
- Validate with UaExpert or another OPC UA client.

Minimum node set:

```text
SDPLC/tank.level
SDPLC/tank.air_flow
SDPLC/tank.feed_flow
SDPLC/tank.tailings_flow
SDPLC/tank.concentrate_grade
SDPLC/tank.emergency_stop
SDPLC/tank.motor_running
SDPLC/runtime.cycle
SDPLC/runtime.avg_exec_us
SDPLC/runtime.max_jitter_us
```

Definition of done:

- Server starts locally.
- Client can browse namespace.
- Client can read all variables.
- At least one writable variable can be modified from the client and reflected in the process image.

Evidence to save:

```text
results/opcua_browse_screenshot.png
results/opcua_read_values.csv
results/opcua_test_notes.md
```

### Sprint 4: Thesis-grade validation

Target: before Sunday.

Deliverables:

- Run three benchmark groups:
  1. compiler pipeline benchmark;
  2. runtime scan-cycle benchmark;
  3. OPC UA read/write response benchmark.
- Produce repeatable CSV files.
- Produce Markdown result summaries suitable for thesis conversion.

Minimum benchmark matrix:

| Test | Scan time | Cycles | Output |
|---|---:|---:|---|
| Flotation runtime | 10 ms | 1000 | timing CSV |
| Flotation runtime | 20 ms | 1000 | timing CSV |
| Flotation runtime | 50 ms | 1000 | timing CSV |
| Control flow runtime | 10 ms | 1000 | timing CSV |
| OPC UA read loop | n/a | 1000 reads | latency CSV |
| OPC UA write loop | n/a | 100 writes | latency CSV |

Definition of done:

```text
results/
  scan_timing_10ms.csv
  scan_timing_20ms.csv
  scan_timing_50ms.csv
  flotation_tank_telemetry.csv
  opcua_read_latency.csv
  opcua_write_latency.csv
  validation_summary.md
```

## Thesis positioning

The final thesis should claim the following achieved contributions only if evidence exists in `results/`:

1. A Rust IEC 61131-3 Structured Text compiler frontend.
2. Semantic checking and LLVM IR generation for representative ST constructs.
3. A deterministic scan-cycle runtime prototype.
4. A flotation-tank simulated I/O validation case study.
5. OPC UA read/write exposure of runtime variables.

Do not claim full O-PAS compliance. Claim alignment with O-PAS architectural direction through OPC UA connectivity and vendor-agnostic execution.

## Cut lines

If time runs short, cut in this order:

1. WebAssembly.
2. CODESYS PLCopen XML ingestion.
3. Advanced OPC UA security.
4. Function block process-image export.
5. Multi-language LD/FBD/SFC support.

Do not cut:

1. ST compilation evidence.
2. Runtime timing evidence.
3. Flotation validation artefacts.
4. OPC UA browsable/readable variable evidence.

## Sunday-ready outcome

A realistic Sunday demo is:

```text
cargo run --bin sdplc -- examples/flotation_tank.st --emit-ir
cargo run --bin runtime -- examples/flotation_tank.st --cycles=1000 --scan-time=10 -q
cargo run --bin opcua_server -- examples/flotation_tank.st --scan-time=10
```

Plus a short results pack showing:

- compiler success;
- runtime scan-cycle measurements;
- flotation tank telemetry trends;
- OPC UA client browse/read evidence.
