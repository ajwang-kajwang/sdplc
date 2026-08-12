//! # SD-PLC
//!
//! An IEC 61131-3 Structured Text compiler targeting LLVM IR
//! for cross-platform industrial control.
//!
//! This crate implements the complete compiler frontend and LLVM
//! code generation pipeline: **lexer** → **parser** → **semantic
//! analysis** → **LLVM IR generation** via `inkwell`.
//!
//! Runtime-facing support modules provide a typed process image,
//! deterministic scan-cycle metrics, a flotation tank simulation
//! harness, and an OPC UA address-space bridge scaffold.
//!
//! [`stdlib`] carries the IEC 61131-3 standard function blocks — the
//! timers, counters, edge detectors and latches — written in Structured
//! Text and compiled through this same pipeline. See `Developer_Guide.md`
//! §13 for how function block instances are laid out and called.

pub mod ast;
pub mod codegen;
pub mod lexer;
pub mod parser;
pub mod semantic;
pub mod stdlib;

pub mod opcua_bridge;
pub mod process_image;
pub mod simulation;
pub mod timing;
