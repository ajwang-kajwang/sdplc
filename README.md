# SD-PLC

A cross-platform IEC 61131-3 Structured Text compiler written in Rust, targeting LLVM IR for native code generation across heterogeneous industrial hardware.

## Overview

SD-PLC compiles IEC 61131-3 Structured Text programs through LLVM to produce native binaries for multiple processor architectures — from x86\_64 development machines down to ARMv5TE industrial IoT boards. The goal is a vendor-agnostic, field-deployable control platform that maintains deterministic execution while enabling modern DevOps workflows.

### Current Status

| Stage | Status |
|-------|--------|
| Lexer | ✅ Complete — full IEC 61131-3:2025 ST coverage |
| Parser / AST | ✅ Complete — recursive descent with precedence climbing |
| Semantic Analysis | ✅ Complete — type resolution, scope validation, LLVM type mapping |
| LLVM IR Generation | ✅ Complete — inkwell codegen for all ST constructs |
| Runtime | ✅ Complete — JIT scan cycle executor with terminal dashboard |
| OPC UA | 🔲 Planned |

### Target Architectures

| Platform | Architecture | Tier |
|----------|-------------|------|
| Development laptop | x86\_64 | Tier 1 |
| Jetson Orin Nano | ARMv8.2-A (Cortex-A78AE) | Tier 1 |
| Raspberry Pi 4 | ARMv8-A (Cortex-A72) | Tier 1 |
| Nuvoton NUC980 | ARMv5TE (ARM926EJ-S) | Tier 2 |


## Building

```bash
cargo build
```

## Running

Compile a Structured Text file:

```bash
# Compile an ST file → output.ll + output.bc
sdplc programs/hello.st

# Custom output name
sdplc programs/pid_controller.st -o build/pid

# Print LLVM IR to stdout (useful for piping to llc)
sdplc programs/flotation_column.st --emit-ir

# Quiet mode (errors only)
sdplc -q programs/hello.st

# Run built-in demo (no arguments)
sdplc
```

### Example Programs

The `examples/` directory contains ready-to-compile ST programs:

| File | Description |
|------|-------------|
| `hello.st` | Minimal counter program — start here |
| `pid_controller.st` | PID function block with anti-windup |
| `multi_pou.st` | Functions, FBs, and programs in one file |
| `all_control_flow.st` | Every ST control flow construct |
| `flotation_column.st` | Rougher flotation column — thesis validation target |

## Runtime (Live Execution)

The runtime JIT-compiles an ST program and executes it in a deterministic scan cycle loop with a live terminal dashboard:

```bash
# Run with default 100ms scan cycle
cargo run --bin runtime -- programs/hello.st

# 50ms scan cycle
cargo run --bin runtime -- programs/flotation_column.st --scan-time=50

# Run exactly 500 cycles then stop
cargo run --bin runtime -- programs/hello.st --cycles=500

# Quiet mode (summary only)
cargo run --bin runtime -- programs/hello.st --cycles=100 -q
```

The dashboard shows all PROGRAM variables updating in real time:

```
══ SD-PLC Runtime ══  ConveyorControl  Scan: 100ms  Cycle: #347
   Uptime: 34.7s  Exec: 1.2µs  Jitter avg: 0.3µs  max: 12.1µs

 Variable                 Type           Value
 ──────────────────────────────────────────────────
 speed                    REAL          67.3200
 running                  BOOL            TRUE
 count                    INT              347
 limit                    INT             1000
 i                        INT                8
```

```

## Project Structure

```
sdplc/
├── Cargo.toml
├── README.md
├── docs/
│   ├── developer_guide.md          # Codebase walkthrough with line numbers
│   └── multi_language_design.md    # Design: LD/FBD/SFC via PLCopen XML
├── examples/
│   ├── hello.st                    # Minimal program
│   ├── pid_controller.st           # PID function block
│   ├── multi_pou.st               # Multiple POUs in one file
│   ├── all_control_flow.st        # Every control flow construct
│   └── flotation_column.st        # Thesis validation target
├── programs/                       # YOUR working .st files
├── src/
│   ├── main.rs         # CLI compiler driver (sdplc binary)
│   ├── bin/
│   │   └── runtime.rs  # JIT scan cycle executor (runtime binary)
│   ├── lib.rs          # Crate root — exports modules
│   ├── ast.rs          # AST node definitions
│   ├── codegen.rs      # LLVM IR generation via inkwell
│   ├── lexer.rs        # IEC 61131-3 ST lexer
│   ├── parser.rs       # Recursive descent parser
│   └── semantic.rs     # Type resolution, scope validation, type checking
└── tests/
    ├── lexer_integration_test.rs
    ├── parser_integration_test.rs
    ├── semantic_integration_test.rs
    └── codegen_integration_test.rs
```

## Compiling to Native Code

After `cargo run` generates `output.ll` and `output.bc`, use LLVM tools to compile for any target:

```bash
# Native (development machine)
llc output.ll -o output.s
gcc output.s -o conveyor

# ARMv8 (Jetson Orin Nano / Raspberry Pi 4)
llc output.ll -mtriple=aarch64-linux-gnu -o output_arm64.s
aarch64-linux-gnu-gcc output_arm64.s -o conveyor_arm64

# ARMv5TE (Nuvoton NUC980)
llc output.ll -mtriple=armv5te-linux-gnueabi -o output_armv5.s

# WebAssembly
llc output.ll -mtriple=wasm32-unknown-unknown -o output.wasm
```

**Same Structured Text source** compiles to native binaries for all four target architectures through a single LLVM IR representation.

## Compilation Pipeline

```
IEC 61131-3 ST Source
        │
        ▼
   ┌─────────┐
   │  Lexer   │  ✅
   └────┬─────┘
        │ Token stream
        ▼
   ┌─────────┐
   │  Parser  │  ✅  → Abstract Syntax Tree
   └────┬─────┘
        │
        ▼
   ┌──────────────┐
   │   Semantic    │  ✅  → Type checking, scope validation
   │   Analysis    │
   └──────┬───────┘
          │
          ▼
   ┌──────────────┐
   │ LLVM Codegen  │  ✅  → Platform-independent IR (inkwell)
   └──────┬───────┘
          │
     ┌────┴────┐
     ▼         ▼
  Native    WebAssembly
  (x86,     (portable,
   ARM)      sandboxed)
```
