# SD-PLC Developer Guide

This guide is written as a mental model first and a file map second. The goal is
to make the whole system feel graspable: a Structured Text program enters as
plain source, becomes tokens, then an AST, then checked program meaning, then
LLVM IR or a scan-cycle runtime, and finally validation evidence in `results/`.

## The One-Screen Picture

```text
Structured Text file
  |
  v
src/lexer.rs
  source characters -> tokens
  |
  v
src/parser.rs + src/ast.rs
  tokens -> CompilationUnit AST
  |
  v
src/semantic.rs
  AST -> checked names, scopes, and types
  |
  +----------------------------+
  |                            |
  v                            v
src/codegen.rs              src/codegen.rs runtime mode
  AST -> LLVM .ll/.bc          AST -> globals + __init + __scan + getters
  |                            |
  v                            v
native/cross compile        src/bin/runtime.rs
                               deterministic scan cycle
                               |
                               v
                            src/process_image.rs
                               typed runtime values
                               |
                  +------------+-------------+
                  |                          |
                  v                          v
          src/simulation.rs           src/bin/opcua_server.rs
          validation plant model      OPC UA address space
                  |                          |
                  +------------+-------------+
                               v
                         results/*
                         CSV, Markdown, IR, bitcode evidence
```

The compiler half answers: "Is this ST program valid, and what LLVM IR does it
mean?"

The runtime/validation half answers: "Can this compiled/control-system model run
in a repeatable scan cycle, expose process variables, and leave evidence?"

## Repository Map

```text
sdplc/
|-- Cargo.toml
|-- README.md
|-- docs/
|   |-- Developer Guide.md
|   `-- sprint_roadmap.md
|-- examples/
|   `-- flotation_tank.st
|-- programs/
|   `-- control_flow.st
|-- src/
|   |-- main.rs              compiler CLI: cargo run --bin sdplc
|   |-- lib.rs               public module list
|   |-- ast.rs               shared syntax tree data model
|   |-- lexer.rs             source text to tokens
|   |-- parser.rs            tokens to AST
|   |-- semantic.rs          AST validation and type resolution
|   |-- codegen.rs           LLVM IR and runtime-JIT code generation
|   |-- process_image.rs     typed variable store for runtime/OPC UA
|   |-- timing.rs            scan-cycle timing metrics
|   |-- simulation.rs        deterministic flotation tank model
|   |-- opcua_bridge.rs      process-image to OPC UA node mapping
|   `-- bin/
|       |-- runtime.rs       JIT scan-cycle executor
|       |-- validate_sim.rs  flotation simulation validation runner
|       |-- opcua_server.rs  OPC UA server and self-test
|       `-- demo_validation.rs validation pack orchestrator
|-- tests/
`-- results/
    |-- compiler_ir/
    |-- compiler_benchmark/
    |-- runtime/
    |-- simulation/
    |-- opcua/
    `-- validation/
```

`results/` is ignored by Git, so it is a local evidence workspace rather than
source code. Its subfolders are described near the end of this guide.

## Stage 1: Source Loading

There are three main entry points:

| Command | File | Purpose |
|---|---|---|
| `cargo run --bin sdplc -- programs/control_flow.st` | `src/main.rs` | Compile ST to `.ll` and `.bc` |
| `cargo run --bin runtime -- programs/control_flow.st --cycles=100` | `src/bin/runtime.rs` | JIT-compile and execute scan cycles |
| `cargo run --bin validate_sim -- --cycles=1000` | `src/bin/validate_sim.rs` | Run the flotation plant model and write evidence |
| `cargo run --bin opcua_server -- examples/flotation_tank.st --self-test` | `src/bin/opcua_server.rs` | Expose process variables through OPC UA |
| `cargo run --bin demo_validation -- --quick` | `src/bin/demo_validation.rs` | Orchestrate compiler, runtime, simulation, and OPC UA evidence |

The compiler and runtime both begin the same way:

1. Parse CLI arguments.
2. Read the `.st` file into a `String`.
3. Derive the source name from the file stem.
4. Pass `&source` into the lexer.

After this point the compiler pipeline does not care where the source came from.
That is a useful design boundary: file I/O is at the edges; compiler logic works
on memory structures.

## Stage 2: Lexer (`src/lexer.rs`)

The lexer is the system's scanner. It walks raw characters and classifies them.

Example:

```text
IF count >= limit THEN
```

becomes approximately:

```text
If, Ident("count"), GreaterEq, Ident("limit"), Then
```

Important pieces:

| Piece | Role |
|---|---|
| `TokenType` | The vocabulary of the language: keywords, operators, literals, punctuation |
| `Token` | A single token plus its original text, line, and column |
| `Lexer` | Cursor over the source characters |
| `tokenize()` | Repeatedly calls `next_token()` until EOF |

What the lexer understands:

- IEC-style keywords such as `PROGRAM`, `VAR`, `IF`, `FOR`, `WHILE`, `CASE`.
- Identifiers, integers, reals, strings, and temporal literals.
- Block comments `(* ... *)`, line comments `// ...`, and whitespace.
- Multi-character operators such as `:=`, `<=`, `>=`, `<>`, `..`, and `**`.

The lexer deliberately does not decide whether the program makes sense. It only
says what the text is made of.

## Stage 3: Parser And AST (`src/parser.rs`, `src/ast.rs`)

The parser turns a flat token stream into a tree. This tree is the central
contract of the compiler.

The root is:

```rust
CompilationUnit {
    units: Vec<Pou>
}
```

A `Pou` is a Program Organization Unit:

- `PROGRAM`
- `FUNCTION`
- `FUNCTION_BLOCK`

The parser recognizes the shape of the language:

```text
PROGRAM Control
VAR
    count : INT := 0;
END_VAR

IF count < 10 THEN
    count := count + 1;
END_IF;
END_PROGRAM
```

becomes a `Program` node with:

- variable blocks,
- declarations,
- statements,
- expressions inside those statements.

The AST is intentionally shared. Semantic analysis reads it, code generation
reads it, and future graphical-language front ends could produce it without
going through the ST text parser.

### Expression Precedence

The parser encodes operator precedence by calling deeper parsing functions for
tighter-binding operators:

```text
OR
XOR
AND
= <>
< > <= >=
+ -
* / MOD
**
unary + - NOT
postfix array/member access
primary literals, identifiers, calls, parenthesized expressions
```

That is why `2 + 3 * 4` naturally becomes `2 + (3 * 4)`.

## Stage 4: Semantic Analysis (`src/semantic.rs`)

Semantic analysis asks: "This parsed correctly, but is it meaningful?"

It checks things like:

- Is every variable declared before use?
- Is a variable assignable, or is it `CONSTANT`?
- Does an `IF` or `WHILE` condition resolve to `BOOL`?
- Are arithmetic operators used on compatible numeric types?
- Is `EXIT` inside a loop?
- Are function calls known and called with compatible arguments?

The main output is a `ProgramContext`, which contains:

| Field | Meaning |
|---|---|
| AST | The checked program tree |
| Symbol information | Known POUs, variables, resolved types, qualifiers |
| Diagnostics | Errors and warnings with source positions |

The key mental shift is this: the parser knows that `count + 1` is an addition
expression; semantic analysis knows whether `count` exists and what type the
addition returns.

## Stage 5: LLVM Code Generation (`src/codegen.rs`)

Code generation translates the checked AST into LLVM IR through `inkwell`.

There are two modes.

### Compiler Mode

Used by `src/main.rs`:

```text
AST -> LLVM module -> .ll text file + .bc bitcode file
```

Typical output command:

```bash
cargo run --bin sdplc -- programs/control_flow.st -o results/compiler_ir/control_flow/control_flow
```

This writes:

```text
results/compiler_ir/control_flow/control_flow.ll
results/compiler_ir/control_flow/control_flow.bc
```

In this mode, ordinary variables are local stack allocations inside generated
functions. It is ideal for inspecting compiler output and using LLVM tools such
as `llc`.

### Runtime Mode

Used by `src/bin/runtime.rs`:

```text
AST -> LLVM module with persistent globals
```

For a program variable such as `speed`, runtime codegen emits:

```text
@speed              persistent global storage
__init_Program()    one-time initialization
__scan_Program()    one scan-cycle body
__get_Program_speed() getter returning f64 for display/evidence
```

This is the big runtime idea: PLC variables must survive from one scan cycle to
the next. Globals give the JIT-compiled scan function persistent memory.

## Stage 6: Runtime Scan Cycle (`src/bin/runtime.rs`)

The runtime is a small PLC-like executor:

1. Read source.
2. Lex, parse, and semantically check it.
3. Compile it in runtime mode.
4. Create an LLVM JIT execution engine.
5. Call `__init_*` once.
6. Repeatedly call `__scan_*`.
7. Read generated getters into a `ProcessImage`.
8. Record timing through `ScanTiming`.
9. Write CSV evidence.

The scan loop is:

```text
cycle_start = now
call generated __scan function
measure execution time
refresh process image from generated getters
display values unless quiet
sleep until target scan period
record jitter and execution timing
```

Run it like this:

```bash
cargo run --bin runtime -- programs/control_flow.st --cycles=100 --scan-time=10 --out=results/runtime/control_flow/latest -q
```

The runtime writes:

```text
runtime_scan_timing.csv
runtime_final_values.csv
```

## Process Image (`src/process_image.rs`)

The process image is the shared typed variable store used by the runtime,
simulation, and OPC UA layers.

```text
ProcessImage
  tank.level -> PlcValue::F64(50.0)
  tank.motor_running -> PlcValue::Bool(true)
  runtime.cycle -> PlcValue::U64(100)
```

Important types:

| Type | Role |
|---|---|
| `PlcValue` | Runtime value enum: `Bool`, `I64`, `U64`, `F64`, `Text` |
| `ProcessVariable` | Name, value, writable flag, description |
| `ProcessImage` | Sorted map of process variables |

The writable flag matters because external clients should not be able to change
derived metrics such as `runtime.avg_exec_us` or calculated values such as
`tank.concentrate_grade`.

## Timing (`src/timing.rs`)

`ScanTiming` records repeatable validation metrics:

- cycle count,
- target scan period,
- average and maximum execution time,
- average and maximum jitter,
- uptime.

It also owns the CSV format used by runtime and validation artefacts. Keeping
that format in one module keeps benchmark output consistent.

## Flotation Simulation (`src/simulation.rs`, `src/bin/validate_sim.rs`)

The flotation tank model is a deterministic plant substitute for validation.
It gives the project useful evidence before hardware is involved.

`FlotationTankSim` owns:

- tank level,
- air flow,
- feed flow,
- tailings flow,
- concentrate grade,
- emergency stop,
- motor running.

Each scan step:

1. Reads writable state from the `ProcessImage`.
2. Advances the process model by `dt`.
3. Writes calculated state back to the `ProcessImage`.
4. Produces telemetry rows when running validation.

Run it like this:

```bash
cargo run --bin validate_sim -- --cycles=1000 --scan-time=10 --out=results/simulation/flotation_tank
```

Typical outputs:

```text
scan_timing.csv
flotation_tank_telemetry.csv
opcua_address_space.csv
```

## OPC UA (`src/opcua_bridge.rs`, `src/bin/opcua_server.rs`)

The OPC UA layer exposes process-image variables as browsable nodes.

`opcua_bridge.rs` is the crate-independent mapping:

```text
ProcessVariable "tank.level"
  -> node_id "ns=2;s=SDPLC.tank.level"
  -> browse_name "tank.level"
  -> data_type "Double"
  -> writable true
```

`opcua_server.rs` binds that mapping to the `async-opcua-server` crate. It:

- creates an anonymous OPC UA endpoint,
- installs folders under `Objects/SDPLC`,
- creates variable nodes under `tank` and `runtime`,
- adds read callbacks that read from the shared `ProcessImage`,
- adds write callbacks for writable variables,
- can run a real client self-test against the server.

Run a self-test like this:

```bash
cargo run --bin opcua_server -- examples/flotation_tank.st --scan-time=10 --self-test --out=results/opcua
```

Typical outputs:

```text
opcua_address_space.csv
opcua_read_values.csv
opcua_test_notes.md
opcua_client_smoke.csv
opcua_read_latency.csv
opcua_write_latency.csv
```

## Validation Pack (`src/bin/demo_validation.rs`)

`demo_validation` is the orchestration binary. It runs public commands so the
evidence matches what a reader can repeat.

It measures:

- compiler pipeline phases,
- generated compiler artefacts,
- runtime scan timing at multiple scan periods,
- flotation telemetry,
- OPC UA browse/read/write smoke behavior,
- OPC UA read/write latency.

Development smoke run:

```bash
cargo run --bin demo_validation -- --quick --out=results/validation/latest
```

Fuller run:

```bash
cargo run --bin demo_validation -- --cycles=1000 --out=results/validation/latest
```

Note: `demo_validation` currently writes its own files directly under the
provided `--out` directory. For a clean evidence archive, choose a run-specific
directory such as `results/validation/2026-05-19-full`.

## Results Folder Layout

The local `results/` folder has been split by evidence type and source so files
can be identified without opening them.

```text
results/
|-- compiler_ir/
|   |-- control_flow/
|   |   |-- control_flow.ll
|   |   `-- control_flow.bc
|   `-- flotation_tank/
|       |-- flotation_tank.ll
|       `-- flotation_tank.bc
|-- compiler_benchmark/
|   `-- compiler_pipeline_benchmark.csv
|-- runtime/
|   |-- control_flow/
|   |   `-- control_flow_scan_timing_10ms.csv
|   |-- flotation_tank/
|   |   |-- scan_timing_10ms.csv
|   |   |-- scan_timing_20ms.csv
|   |   `-- scan_timing_50ms.csv
|   `-- latest/
|       |-- runtime_scan_timing.csv
|       `-- runtime_final_values.csv
|-- simulation/
|   `-- flotation_tank/
|       |-- scan_timing.csv
|       `-- flotation_tank_telemetry.csv
|-- opcua/
|   |-- address_space/
|   |   |-- opcua_address_space.csv
|   |   |-- opcua_read_values.csv
|   |   `-- opcua_test_notes.md
|   |-- self_test/
|   |   `-- opcua_client_smoke.csv
|   `-- latency/
|       |-- opcua_read_latency.csv
|       `-- opcua_write_latency.csv
`-- validation/
    `-- validation_summary.md
```

Use these conventions when generating new evidence:

```bash
# Compiler IR for a specific ST file
cargo run --bin sdplc -- programs/control_flow.st -o results/compiler_ir/control_flow/control_flow

# Runtime evidence for a source and scan period
cargo run --bin runtime -- programs/control_flow.st --cycles=100 --scan-time=10 --out=results/runtime/control_flow/latest -q

# Simulation evidence
cargo run --bin validate_sim -- --cycles=1000 --scan-time=10 --out=results/simulation/flotation_tank

# OPC UA evidence
cargo run --bin opcua_server -- examples/flotation_tank.st --scan-time=10 --self-test --out=results/opcua
```

## Common Changes

### Add A Keyword

1. Add a `TokenType` variant in `src/lexer.rs`.
2. Add the uppercase keyword string to the lexer keyword match table.
3. If it changes syntax, add parsing logic in `src/parser.rs`.
4. Add lexer/parser tests.

### Add A Data Type

1. Add the syntax-level type in `src/ast.rs`.
2. Teach the lexer to recognize any new keyword.
3. Teach the parser to produce the new `TypeSpec`.
4. Teach semantic analysis to resolve it to a `ResolvedType`.
5. Teach codegen how to map it to an LLVM type.
6. Add tests at each layer touched.

### Add A Statement

1. Add a `Statement` variant in `src/ast.rs`.
2. Add a parser branch in `parse_statement()`.
3. Add semantic validation in `analyze_statement()`.
4. Add LLVM emission in `emit_statement()`.
5. Add integration tests from ST source to expected behavior or IR.

### Add A New Runtime Metric

1. Add the value to `ScanTiming` if it is timing-related.
2. Refresh it into `ProcessImage` if it should be visible externally.
3. Expose it through OPC UA by inserting a `runtime.*` process variable.
4. Add it to CSV evidence if it supports validation.

### Add A New OPC UA Variable

1. Insert a `ProcessVariable` into the relevant `ProcessImage`.
2. Set `read_only()` if clients should not write it.
3. Give it a description with `with_description()`.
4. The bridge and server will map it into the OPC UA address space.

## How To Debug The Pipeline

Use the stage boundaries to localize bugs:

| Symptom | Start Here |
|---|---|
| Unknown token | `src/lexer.rs` |
| Parse error near valid-looking tokens | `src/parser.rs` |
| Type or declaration error | `src/semantic.rs` |
| LLVM verifier or IR issue | `src/codegen.rs` |
| Runtime values reset every scan | runtime codegen globals in `src/codegen.rs` |
| CSV timing looks wrong | `src/timing.rs` and scan loop sleep logic |
| OPC UA node missing | `ProcessImage` seeding, then `src/opcua_bridge.rs` |
| OPC UA write fails | writable flag and write callback type conversion |

## The Core Design Idea

SD-PLC is easiest to understand as two connected pipelines.

The compile pipeline progressively adds meaning:

```text
text -> tokens -> AST -> checked AST -> LLVM IR
```

The validation pipeline turns execution into evidence:

```text
JIT scan cycle -> process image -> simulation/OPC UA -> CSV and Markdown results
```

The AST is the compiler's shared language. The `ProcessImage` is the runtime's
shared language. Almost every file in the repository either helps build one of
those two structures, consume one of them, or turn them into evidence.
