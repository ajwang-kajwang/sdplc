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
| LLVM IR Generation | 🔲 Next |
| Runtime | 🔲 Planned |
| OPC UA | 🔲 Planned |


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
│   ├── main.rs         # CLI entry point (3-stage frontend demo)
│   ├── ast.rs          # AST node definitions
│   ├── lexer.rs        # IEC 61131-3 ST lexer
│   ├── parser.rs       # Recursive descent parser
│   └── semantic.rs     # Type resolution, scope validation, type checking
└── tests/
    ├── lexer_integration_test.rs      # Full-program lexer tests
    ├── parser_integration_test.rs     # Full-program parser tests
    └── semantic_integration_test.rs   # Type checking / validation tests
```

## Compilation Pipeline (Planned)

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

