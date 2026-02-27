# SD-PLC

A cross-platform IEC 61131-3 Structured Text compiler written in Rust, targeting LLVM IR for native code generation across heterogeneous industrial hardware.

## Overview

SD-PLC compiles IEC 61131-3 Structured Text programs through LLVM to produce native binaries for multiple processor architectures — from x86\_64 development machines down to ARMv5TE industrial IoT boards. The goal is a vendor-agnostic, field-deployable control platform that maintains deterministic execution while enabling modern DevOps workflows.

### Current Status

| Stage | Status |
|-------|--------|
| Lexer | ✅ Complete — full IEC 61131-3:2025 ST coverage |
| Parser / AST | 🔲 Next |
| Semantic Analysis | 🔲 Planned |
| LLVM IR Generation | 🔲 Planned |
| Runtime | 🔲 Planned |
| OPC UA / Django IDE | 🔲 Planned |

### Target Architectures

| Platform | Architecture | Tier |
|----------|-------------|------|
| Development laptop | x86\_64 | Tier 1 |
| Jetson Orin Nano | ARMv8.2-A (Cortex-A78AE) | Tier 1 |
| Raspberry Pi 4 | ARMv8-A (Cortex-A72) | Tier 1 |
| Nuvoton NUC980 | ARMv5TE (ARM926EJ-S) | Tier 2 |

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

Generate documentation
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
│   ├── main.rs         # CLI entry point (lexer demo)
│   └── lexer.rs        # IEC 61131-3 ST lexer
└── tests/
    └── lexer_integration_test.rs   # Full-program lexer tests
```

## Compilation Pipeline 

```
IEC 61131-3 ST Source
        │
        ▼
   ┌─────────┐
   │  Lexer   │  ← you are here
   └────┬─────┘
        │ Token stream
        ▼
   ┌─────────┐
   │  Parser  │  → Abstract Syntax Tree
   └────┬─────┘
        │
        ▼
   ┌──────────────┐
   │   Semantic    │  → Type checking, scope validation
   │   Analysis    │
   └──────┬───────┘
          │
          ▼
   ┌──────────────┐
   │  LLVM IR Gen  │  → Platform-independent IR (inkwell)
   └──────┬───────┘
          │
     ┌────┴────┐
     ▼         ▼
  Native    WebAssembly
  (x86,     (portable,
   ARM)      sandboxed)
```
