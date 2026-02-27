# SD-PLC

An IEC 61131-3 Structured Text compiler written in Rust. Currently, this project implements a lexer capable of tokenizing Structured Text according to the IEC 61131-3:2025 standard.

## Prerequisites

This project uses LLVM as a backend via the `inkwell` crate. You must have LLVM 17 installed.
