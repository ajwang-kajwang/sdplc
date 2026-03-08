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
| Runtime | 🔲 Next — deterministic scan cycle executor |
| OPC UA | 🔲 Planned |

### Target Architectures

| Platform | Architecture | Tier |
|----------|-------------|------|
| Development laptop | x86\_64 | Tier 1 |
| Jetson Orin Nano | ARMv8.2-A (Cortex-A78AE) | Tier 1 |
| Raspberry Pi 4 | ARMv8-A (Cortex-A72) | Tier 1 |
| Nuvoton NUC980 | ARMv5TE (ARM926EJ-S) | Tier 2 |

## Prerequisites

### Rust Toolchain

Install via [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### LLVM 17

The `inkwell` crate requires LLVM 17 development libraries.

**Ubuntu / Debian:**

```bash
wget https://apt.llvm.org/llvm.sh
chmod +x llvm.sh
sudo ./llvm.sh 17
sudo apt install llvm-17-dev libpolly-17-dev
```

**macOS (Homebrew):**

```bash
brew install llvm@17
```

**Windows:**

The project looks for LLVM 17 at the path configured in `.cargo/config.toml`. Update the path if your installation differs:

```toml
# .cargo/config.toml
[env]
LLVM_SYS_170_PREFIX = "C:\\Users\\kajwa\\Development\\llvm-install"
```

## Building

```bash
cargo build
```

## Running

The binary currently runs the lexer against a sample Conveyor Control program and prints the token table:

```bash
cargo run
```

## Testing

Unit tests are embedded in `src/lexer.rs`. Integration tests live in `tests/`.

```bash
# Run all tests
cargo test

# Run only unit tests
cargo test --lib

# Run only integration tests
cargo test --test lexer_integration_test
```

## Documentation

Generate and open HTML documentation (includes rustdoc examples):

```bash
cargo doc --open
```

## Project Structure

```
sdplc/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs          # Crate root — exports modules
│   ├── main.rs         # CLI entry point (4-stage compiler demo)
│   ├── ast.rs          # AST node definitions
│   ├── codegen.rs      # LLVM IR generation via inkwell
│   ├── lexer.rs        # IEC 61131-3 ST lexer
│   ├── parser.rs       # Recursive descent parser
│   └── semantic.rs     # Type resolution, scope validation, type checking
└── tests/
    ├── lexer_integration_test.rs      # Full-program lexer tests
    ├── parser_integration_test.rs     # Full-program parser tests
    ├── semantic_integration_test.rs   # Type checking / validation tests
    └── codegen_integration_test.rs    # LLVM IR output verification tests
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

This is the thesis claim made concrete: the **same Structured Text source** compiles to native binaries for all four target architectures through a single LLVM IR representation.

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
