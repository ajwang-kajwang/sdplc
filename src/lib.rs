//! # SD-PLC
//!
//! An IEC 61131-3 Structured Text compiler targeting LLVM IR
//! for cross-platform industrial control.
//!
//! This crate implements the complete compiler frontend and LLVM
//! code generation pipeline: **lexer** → **parser** → **semantic
//! analysis** → **LLVM IR generation** via `inkwell`.
//!
//! The current completion sprint adds runtime-facing support modules:
//! a typed process image, deterministic scan-cycle metrics, a flotation
//! tank simulation harness, and an OPC UA address-space bridge scaffold.

pub mod ast;
pub mod codegen;
pub mod lexer;
pub mod parser;
pub mod semantic;

pub mod opcua_bridge;
pub mod process_image;
pub mod simulation;
pub mod timing;
