//! LLVM IR code generation for IEC 61131-3 Structured Text.
//!
//! This module walks a semantically validated AST and emits LLVM IR
//! via the `inkwell` crate. The generated IR can then be compiled to
//! native machine code for any LLVM-supported target architecture
//! (x86_64, ARMv8, ARMv5TE, WebAssembly, etc.).
//!
//! ## Design
//!
//! - Each `PROGRAM` becomes a void LLVM function `@ProgramName()`.
//! - Each `FUNCTION` becomes an LLVM function with typed parameters
//!   and return value.
//! - Each `FUNCTION_BLOCK` becomes a void function taking a pointer
//!   to an auto-generated struct holding its variables.
//! - Local variables are stack-allocated with `alloca`.
//! - Expressions are emitted as SSA values using the LLVM builder.
//! - Control flow (IF, FOR, WHILE, REPEAT, CASE) maps to basic
//!   blocks with conditional/unconditional branches.
//!

use std::collections::HashMap;
use std::path::Path;

use inkwell::AddressSpace;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, StructType};
use inkwell::values::{BasicValueEnum, FloatValue, FunctionValue, IntValue, PointerValue};
use inkwell::{FloatPredicate, IntPredicate};

use crate::ast::*;
use crate::semantic::ResolvedType;
use crate::stdlib;

// ─── Runtime support symbols ────────────────────────────────────

/// Module global holding the runtime's monotonic scan clock, in
/// milliseconds. Emitted on demand — only programs that use timers or
/// call `TIME_MS()` carry it.
pub const TIME_GLOBAL: &str = "__sdplc_now_ms";

/// Symbol the host calls once per scan cycle to advance [`TIME_GLOBAL`]
/// before executing the scan body: `void(i64 millis)`.
pub const TIME_SETTER_FN: &str = "__sdplc_set_time_ms";

// ─── Codegen Error ──────────────────────────────────────────────

/// For errors encountered during code generation.
#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "codegen error: {}", self.message)
    }
}

type CgResult<T> = Result<T, CodegenError>;

// ─── Variable Metadata ──────────────────────────────────────────

/// Tracks a declared variable's stack allocation and resolved type.
#[derive(Clone)]
struct VarSlot<'ctx> {
    /// Pointer to the stack allocation (`alloca`).
    ptr: PointerValue<'ctx>,
    /// The resolved type — determines which load/store ops to use.
    resolved_type: ResolvedType,
    /// The LLVM type for loads.
    llvm_type: BasicTypeEnum<'ctx>,
}

// ─── Function Block Instance Layout ─────────────────────────────

/// One field of a function block's instance struct.
///
/// Every declaration in the block — inputs, outputs and internal `VAR`
/// state alike — becomes a field, in declaration order. That is what
/// makes state survive between scan cycles: the fields live in the
/// caller's instance memory, not in the block function's stack frame.
#[derive(Debug, Clone)]
struct FbField {
    name: String,
    /// Index of this field within the instance struct.
    index: u32,
    resolved_type: ResolvedType,
    /// VAR_INPUT / VAR_OUTPUT / VAR — decides how a call site may bind it.
    qualifier: VarQualifier,
    initial_value: Option<Expression>,
}

/// The instance-struct layout of one `FUNCTION_BLOCK`.
#[derive(Debug, Clone)]
struct FbLayout<'ctx> {
    struct_type: StructType<'ctx>,
    fields: Vec<FbField>,
}

impl<'ctx> FbLayout<'ctx> {
    /// Looks up a field by name, case-insensitively (ST is not
    /// case-sensitive in parameter names).
    fn field(&self, name: &str) -> Option<&FbField> {
        self.fields
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name))
    }

    fn inputs(&self) -> impl Iterator<Item = &FbField> {
        self.fields
            .iter()
            .filter(|f| matches!(f.qualifier, VarQualifier::VarInput | VarQualifier::VarInOut))
    }

    fn outputs(&self) -> impl Iterator<Item = &FbField> {
        self.fields
            .iter()
            .filter(|f| matches!(f.qualifier, VarQualifier::VarOutput | VarQualifier::VarInOut))
    }
}

// ─── Runtime Variable Metadata ──────────────────────────────────

/// Metadata for a variable exposed to the runtime.
///
/// The runtime uses this to know which getter/setter functions
/// exist and how to format the values for display.
#[derive(Debug, Clone)]
pub struct RuntimeVar {
    /// The variable name as declared in ST.
    pub name: String,
    /// The resolved type (for formatting).
    pub resolved_type: ResolvedType,
    /// The name of the PROGRAM this variable belongs to.
    pub program_name: String,
    /// Runtime getter symbol. The getter returns the variable as f64.
    pub getter_fn_name: String,
    /// Runtime setter symbol. Present for scalar writable values.
    pub setter_fn_name: Option<String>,
}

// ─── Code Generator ─────────────────────────────────────────────

/// LLVM IR code generator for IEC 61131-3 Structured Text.
///
/// Owns the LLVM module and builder. Call [`compile`](Self::compile)
/// to process an AST, then use [`print_ir`](Self::print_ir),
/// [`write_ir`](Self::write_ir), or [`write_bitcode`](Self::write_bitcode)
/// to output the result.
pub struct CodeGenerator<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,

    /// Current function being generated.
    current_fn: Option<FunctionValue<'ctx>>,
    /// Variable name → stack slot mapping (per-POU, rebuilt each POU).
    variables: HashMap<String, VarSlot<'ctx>>,
    /// Declared LLVM functions for inter-POU calls.
    functions: HashMap<String, FunctionValue<'ctx>>,

    /// Instance-struct layout per FUNCTION_BLOCK name.
    fb_layouts: HashMap<String, FbLayout<'ctx>>,
    /// Per-FUNCTION_BLOCK instance initialiser, `void(ptr self)`. Kept
    /// out of `functions` so ST code cannot call it by name.
    fb_initializers: HashMap<String, FunctionValue<'ctx>>,

    /// Stack of loop-exit basic blocks (for EXIT statements).
    loop_exit_stack: Vec<BasicBlock<'ctx>>,
}

/// Where a runtime-exposed value lives, so the generated getter and
/// setter can reach it: either a global directly, or a field inside a
/// function block instance global.
#[derive(Clone)]
struct RuntimeTarget<'ctx> {
    base: PointerValue<'ctx>,
    /// `Some((instance struct, field index))` for FB instance members.
    member: Option<(StructType<'ctx>, u32)>,
    resolved_type: ResolvedType,
    llvm_type: BasicTypeEnum<'ctx>,
}

impl<'ctx> CodeGenerator<'ctx> {
    /// Creates a new code generator with an empty LLVM module.
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();

        CodeGenerator {
            context,
            module,
            builder,
            current_fn: None,
            variables: HashMap::new(),
            functions: HashMap::new(),
            fb_layouts: HashMap::new(),
            fb_initializers: HashMap::new(),
            loop_exit_stack: Vec::new(),
        }
    }

    // ── Output methods ──────────────────────────────────────────

    /// Prints the generated LLVM IR to stdout.
    pub fn print_ir(&self) {
        self.module.print_to_stderr();
    }

    /// Returns the LLVM IR as a string.
    pub fn ir_string(&self) -> String {
        self.module.print_to_string().to_string()
    }

    /// Writes LLVM IR text to a `.ll` file.
    pub fn write_ir(&self, path: &str) -> CgResult<()> {
        self.module
            .print_to_file(Path::new(path))
            .map_err(|e| CodegenError {
                message: e.to_string(),
            })
    }

    /// Writes LLVM bitcode to a `.bc` file.
    pub fn write_bitcode(&self, path: &str) -> bool {
        self.module.write_bitcode_to_path(Path::new(path))
    }

    /// Returns a reference to the LLVM module (for target machine use).
    pub fn module(&self) -> &Module<'ctx> {
        &self.module
    }

    // ── Type mapping ────────────────────────────────────────────

    /// Maps an AST [`ElementaryType`] to a [`ResolvedType`].
    fn resolve_elementary(et: &ElementaryType) -> ResolvedType {
        match et {
            ElementaryType::Bool => ResolvedType::Bool,
            ElementaryType::Sint => ResolvedType::SignedInt { bits: 8 },
            ElementaryType::Int => ResolvedType::SignedInt { bits: 16 },
            ElementaryType::Dint => ResolvedType::SignedInt { bits: 32 },
            ElementaryType::Lint => ResolvedType::SignedInt { bits: 64 },
            ElementaryType::Usint => ResolvedType::UnsignedInt { bits: 8 },
            ElementaryType::Uint => ResolvedType::UnsignedInt { bits: 16 },
            ElementaryType::Udint => ResolvedType::UnsignedInt { bits: 32 },
            ElementaryType::Ulint => ResolvedType::UnsignedInt { bits: 64 },
            ElementaryType::Real => ResolvedType::Float { bits: 32 },
            ElementaryType::Lreal => ResolvedType::Float { bits: 64 },
            ElementaryType::Byte => ResolvedType::BitString { bits: 8 },
            ElementaryType::Word => ResolvedType::BitString { bits: 16 },
            ElementaryType::Dword => ResolvedType::BitString { bits: 32 },
            ElementaryType::Lword => ResolvedType::BitString { bits: 64 },
            ElementaryType::Time => ResolvedType::Time,
            ElementaryType::Date => ResolvedType::Date,
            ElementaryType::Tod => ResolvedType::Tod,
            ElementaryType::Dt => ResolvedType::Dt,
        }
    }

    /// Maps a [`TypeSpec`] to a [`ResolvedType`].
    fn resolve_type(ts: &TypeSpec) -> ResolvedType {
        match ts {
            TypeSpec::Elementary(et) => Self::resolve_elementary(et),
            TypeSpec::Array {
                ranges,
                element_type,
            } => ResolvedType::Array {
                element: Box::new(Self::resolve_type(element_type)),
                ranges: ranges.clone(),
            },
            TypeSpec::StringType { max_len } => ResolvedType::Str {
                max_len: max_len.unwrap_or(254),
            },
            TypeSpec::WStringType { max_len } => ResolvedType::WStr {
                max_len: max_len.unwrap_or(254),
            },
            TypeSpec::UserDefined(name) => ResolvedType::UserDefined { name: name.clone() },
        }
    }

    /// Converts a [`ResolvedType`] to the corresponding inkwell
    /// [`BasicTypeEnum`].
    fn llvm_type(&self, rt: &ResolvedType) -> BasicTypeEnum<'ctx> {
        match rt {
            ResolvedType::Bool => self.context.bool_type().into(),
            ResolvedType::SignedInt { bits }
            | ResolvedType::UnsignedInt { bits }
            | ResolvedType::BitString { bits } => self.context.custom_width_int_type(*bits).into(),
            ResolvedType::Float { bits: 32 } => self.context.f32_type().into(),
            ResolvedType::Float { bits: 64 } => self.context.f64_type().into(),
            ResolvedType::Float { bits } => {
                // Fallback — shouldn't happen for valid ST
                self.context.custom_width_int_type(*bits).into()
            }
            ResolvedType::Time | ResolvedType::Date | ResolvedType::Tod | ResolvedType::Dt => {
                // Temporal types stored as i64
                self.context.i64_type().into()
            }
            ResolvedType::Str { max_len } => self.context.i8_type().array_type(*max_len + 1).into(),
            ResolvedType::WStr { max_len } => {
                self.context.i16_type().array_type(*max_len + 1).into()
            }
            ResolvedType::Array { element, ranges } => {
                let elem_ty = self.llvm_type(element);
                let total_size: u32 = ranges.iter().map(|r| (r.high - r.low + 1) as u32).product();
                elem_ty.array_type(total_size).into()
            }
            ResolvedType::UserDefined { name } => match self.fb_layouts.get(name) {
                // A function block instance is its layout struct, held
                // inline by the declaring POU.
                Some(layout) => layout.struct_type.into(),
                // Unknown user type — no layout to lay down.
                None => self.context.i64_type().into(),
            },
            ResolvedType::Error => self.context.i32_type().into(),
        }
    }

    // ── Top-level compilation ───────────────────────────────────

    /// Compiles an entire compilation unit to LLVM IR.
    ///
    /// Pass 1: declare all function signatures.
    /// Pass 2: emit bodies.
    pub fn compile(&mut self, unit: &CompilationUnit) -> CgResult<()> {
        // Standard function blocks the program instantiates are compiled
        // alongside it — see `stdlib::inject`.
        let unit = stdlib::inject(unit);

        // Pass 0: name every FB instance struct before any is laid out,
        // so blocks may hold instances of each other.
        self.predeclare_fb_types(&unit);
        // Pass 1: declare function signatures
        for pou in &unit.units {
            self.declare_pou(pou)?;
        }
        // Pass 2: emit bodies
        for pou in &unit.units {
            self.emit_pou(pou)?;
        }
        Ok(())
    }

    /// Creates an empty (opaque) LLVM struct type for every
    /// `FUNCTION_BLOCK` in the unit.
    ///
    /// Bodies are filled in later, by [`declare_pou`](Self::declare_pou).
    /// Splitting the two lets a block declare an instance of a block
    /// defined further down the file.
    fn predeclare_fb_types(&mut self, unit: &CompilationUnit) {
        for pou in &unit.units {
            if let Pou::FunctionBlock(fb) = pou {
                if self.fb_layouts.contains_key(&fb.name) {
                    continue;
                }
                let struct_type = self.context.opaque_struct_type(&format!("FB.{}", fb.name));
                self.fb_layouts.insert(
                    fb.name.clone(),
                    FbLayout {
                        struct_type,
                        fields: Vec::new(),
                    },
                );
            }
        }
    }

    /// Fills in the instance struct body for one `FUNCTION_BLOCK` and
    /// records the field table used by call sites and member access.
    fn layout_function_block(&mut self, fb: &FunctionBlockDecl) {
        let mut fields: Vec<FbField> = Vec::new();
        let mut field_types: Vec<BasicTypeEnum<'ctx>> = Vec::new();

        for block in &fb.var_blocks {
            for decl in &block.declarations {
                let resolved_type = Self::resolve_type(&decl.type_spec);
                field_types.push(self.llvm_type(&resolved_type));
                fields.push(FbField {
                    name: decl.name.clone(),
                    index: fields.len() as u32,
                    resolved_type,
                    qualifier: block.qualifier.clone(),
                    initial_value: decl.initial_value.clone(),
                });
            }
        }

        let struct_type = self.fb_layouts[&fb.name].struct_type;
        struct_type.set_body(&field_types, false);
        self.fb_layouts.insert(
            fb.name.clone(),
            FbLayout {
                struct_type,
                fields,
            },
        );
    }

    /// The symbol of a function block's instance initialiser.
    fn fb_init_name(fb_name: &str) -> String {
        format!("__fbinit_{}", fb_name)
    }

    /// A pointer type. Pointers are opaque under LLVM 15+, so the
    /// pointee is irrelevant — `i8*` is just the conventional spelling.
    fn ptr_type(&self) -> inkwell::types::PointerType<'ctx> {
        self.context.i8_type().ptr_type(AddressSpace::default())
    }

    /// Declares the LLVM function signature for a POU (no body yet).
    fn declare_pou(&mut self, pou: &Pou) -> CgResult<()> {
        match pou {
            Pou::Program(p) => {
                let fn_type = self.context.void_type().fn_type(&[], false);
                let function = self.module.add_function(&p.name, fn_type, None);
                self.functions.insert(p.name.clone(), function);
            }
            Pou::Function(f) => {
                let ret_rt = Self::resolve_type(&f.return_type);
                let ret_ty = self.llvm_type(&ret_rt);

                // Collect input parameters
                let mut param_types: Vec<BasicMetadataTypeEnum<'ctx>> = Vec::new();
                for block in &f.var_blocks {
                    if block.qualifier == VarQualifier::VarInput {
                        for decl in &block.declarations {
                            let rt = Self::resolve_type(&decl.type_spec);
                            param_types.push(self.llvm_type(&rt).into());
                        }
                    }
                }

                let fn_type = ret_ty.fn_type(&param_types, false);
                let function = self.module.add_function(&f.name, fn_type, None);

                // Name the parameters
                let mut idx = 0;
                for block in &f.var_blocks {
                    if block.qualifier == VarQualifier::VarInput {
                        for decl in &block.declarations {
                            if let Some(param) = function.get_nth_param(idx) {
                                param.set_name(&decl.name);
                            }
                            idx += 1;
                        }
                    }
                }

                self.functions.insert(f.name.clone(), function);
            }
            Pou::FunctionBlock(fb) => {
                // A function block is `void @Name(ptr %self)` — all of
                // its state lives in the caller's instance struct, which
                // is what makes it persist across scan cycles.
                self.layout_function_block(fb);

                let fn_type = self
                    .context
                    .void_type()
                    .fn_type(&[self.ptr_type().into()], false);

                let function = self.module.add_function(&fb.name, fn_type, None);
                if let Some(param) = function.get_first_param() {
                    param.set_name("self");
                }
                self.functions.insert(fb.name.clone(), function);

                let init_fn =
                    self.module
                        .add_function(&Self::fb_init_name(&fb.name), fn_type, None);
                if let Some(param) = init_fn.get_first_param() {
                    param.set_name("self");
                }
                self.fb_initializers.insert(fb.name.clone(), init_fn);
            }
        }
        Ok(())
    }

    /// Binds every field of a function block instance as a variable slot
    /// pointing into `self_ptr`, so the block's body reads and writes
    /// instance memory rather than a stack frame.
    ///
    /// The builder must already be positioned in the block where the
    /// field pointers should be computed.
    fn bind_fb_fields(
        &mut self,
        layout: &FbLayout<'ctx>,
        self_ptr: PointerValue<'ctx>,
    ) -> CgResult<()> {
        for field in &layout.fields {
            let ptr = self
                .builder
                .build_struct_gep(layout.struct_type, self_ptr, field.index, &field.name)
                .map_err(|e| CodegenError {
                    message: format!("cannot address field '{}': {}", field.name, e),
                })?;
            let llvm_type = self.llvm_type(&field.resolved_type);
            self.variables.insert(
                field.name.clone(),
                VarSlot {
                    ptr,
                    resolved_type: field.resolved_type.clone(),
                    llvm_type,
                },
            );
        }
        Ok(())
    }

    /// Emits `void @__fbinit_<Name>(ptr %self)` — zeroes every field of
    /// an instance, then applies any declared initial values.
    ///
    /// Callers invoke this once per instance: at the declaration site
    /// for stack instances, and from `__init_<Program>` for the runtime's
    /// module-global instances.
    fn emit_fb_initializer(&mut self, fb: &FunctionBlockDecl) -> CgResult<()> {
        let function = self.fb_initializers[&fb.name];
        let layout = self.fb_layouts[&fb.name].clone();

        self.current_fn = Some(function);
        self.variables.clear();

        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        let self_ptr = function
            .get_first_param()
            .expect("function block initialiser takes an instance pointer")
            .into_pointer_value();
        self.bind_fb_fields(&layout, self_ptr)?;

        for field in &layout.fields {
            let slot = self.variables[&field.name].clone();
            let value = match &field.initial_value {
                Some(expr) => {
                    let raw = self.emit_expression(expr)?;
                    self.coerce_value(raw, &field.resolved_type)
                }
                None => self.zero_value(&field.resolved_type),
            };
            self.builder.build_store(slot.ptr, value).unwrap();
        }

        self.builder.build_return(None).unwrap();
        self.current_fn = None;
        Ok(())
    }

    /// Emits the call that initialises one function block instance.
    fn emit_fb_init_call(&mut self, fb_name: &str, instance_ptr: PointerValue<'ctx>) {
        if let Some(init_fn) = self.fb_initializers.get(fb_name).copied() {
            self.builder
                .build_call(init_fn, &[instance_ptr.into()], "")
                .unwrap();
        }
    }

    /// Returns the function block layout for a slot holding an instance.
    fn instance_layout(&self, slot: &VarSlot<'ctx>) -> Option<(String, FbLayout<'ctx>)> {
        match &slot.resolved_type {
            ResolvedType::UserDefined { name } => self
                .fb_layouts
                .get(name)
                .map(|layout| (name.clone(), layout.clone())),
            _ => None,
        }
    }

    /// Emits the body of a POU.
    fn emit_pou(&mut self, pou: &Pou) -> CgResult<()> {
        match pou {
            Pou::Program(p) => {
                let function = self.functions[&p.name];
                self.current_fn = Some(function);
                self.variables.clear();

                let entry = self.context.append_basic_block(function, "entry");
                self.builder.position_at_end(entry);

                self.emit_var_blocks(&p.var_blocks)?;
                self.emit_statements(&p.body)?;

                // Ensure the function terminates
                if self.needs_terminator() {
                    self.builder.build_return(None).unwrap();
                }

                self.current_fn = None;
            }
            Pou::Function(f) => {
                let function = self.functions[&f.name];
                self.current_fn = Some(function);
                self.variables.clear();

                let entry = self.context.append_basic_block(function, "entry");
                self.builder.position_at_end(entry);

                // Allocate return value slot
                let ret_rt = Self::resolve_type(&f.return_type);
                let ret_llvm_ty = self.llvm_type(&ret_rt);
                let ret_ptr = self.builder.build_alloca(ret_llvm_ty, &f.name).unwrap();
                self.variables.insert(
                    f.name.clone(),
                    VarSlot {
                        ptr: ret_ptr,
                        resolved_type: ret_rt.clone(),
                        llvm_type: ret_llvm_ty,
                    },
                );

                // Copy input params to local allocas
                let mut param_idx = 0u32;
                for block in &f.var_blocks {
                    if block.qualifier == VarQualifier::VarInput {
                        for decl in &block.declarations {
                            let rt = Self::resolve_type(&decl.type_spec);
                            let lt = self.llvm_type(&rt);
                            let ptr = self.builder.build_alloca(lt, &decl.name).unwrap();

                            let param_val = function.get_nth_param(param_idx).unwrap();
                            self.builder.build_store(ptr, param_val).unwrap();

                            self.variables.insert(
                                decl.name.clone(),
                                VarSlot {
                                    ptr,
                                    resolved_type: rt,
                                    llvm_type: lt,
                                },
                            );
                            param_idx += 1;
                        }
                    }
                }

                // Allocate remaining vars
                self.emit_var_blocks_except_inputs(&f.var_blocks)?;

                // Emit body
                self.emit_statements(&f.body)?;

                // Load and return the return value
                if self.needs_terminator() {
                    let ret_val = self
                        .builder
                        .build_load(ret_llvm_ty, ret_ptr, "retval")
                        .unwrap();
                    self.builder.build_return(Some(&ret_val)).unwrap();
                }

                self.current_fn = None;
            }
            Pou::FunctionBlock(fb) => {
                let function = self.functions[&fb.name];
                let layout = self.fb_layouts[&fb.name].clone();

                self.current_fn = Some(function);
                self.variables.clear();

                let entry = self.context.append_basic_block(function, "entry");
                self.builder.position_at_end(entry);

                // No allocas here: the block's variables *are* the
                // caller's instance fields, which is how a TON keeps
                // counting across scan cycles.
                let self_ptr = function
                    .get_first_param()
                    .expect("function block takes an instance pointer")
                    .into_pointer_value();
                self.bind_fb_fields(&layout, self_ptr)?;

                self.emit_statements(&fb.body)?;

                if self.needs_terminator() {
                    self.builder.build_return(None).unwrap();
                }

                self.current_fn = None;

                self.emit_fb_initializer(fb)?;
            }
        }
        Ok(())
    }

    /// Returns true if the current basic block needs a terminator.
    fn needs_terminator(&self) -> bool {
        let bb = self.builder.get_insert_block().unwrap();
        bb.get_terminator().is_none()
    }

    // ── Variable allocation ─────────────────────────────────────

    /// Emits alloca + optional initial store for all var blocks.
    fn emit_var_blocks(&mut self, blocks: &[VarBlock]) -> CgResult<()> {
        for block in blocks {
            for decl in &block.declarations {
                self.emit_var_decl(decl)?;
            }
        }
        Ok(())
    }

    /// Like emit_var_blocks but skips VAR_INPUT (already handled
    /// as function parameters).
    fn emit_var_blocks_except_inputs(&mut self, blocks: &[VarBlock]) -> CgResult<()> {
        for block in blocks {
            if block.qualifier == VarQualifier::VarInput {
                continue;
            }
            for decl in &block.declarations {
                self.emit_var_decl(decl)?;
            }
        }
        Ok(())
    }

    /// Emits a single variable declaration: alloca + optional init.
    fn emit_var_decl(&mut self, decl: &VarDecl) -> CgResult<()> {
        let rt = Self::resolve_type(&decl.type_spec);
        let lt = self.llvm_type(&rt);
        let ptr = self.builder.build_alloca(lt, &decl.name).unwrap();

        // A function block instance is initialised by its own generated
        // initialiser, which knows the struct's field defaults.
        if let ResolvedType::UserDefined { name: fb_name } = &rt {
            if self.fb_layouts.contains_key(fb_name) {
                let fb_name = fb_name.clone();
                self.emit_fb_init_call(&fb_name, ptr);
                self.variables.insert(
                    decl.name.clone(),
                    VarSlot {
                        ptr,
                        resolved_type: rt,
                        llvm_type: lt,
                    },
                );
                return Ok(());
            }
        }

        // Store initial value if present
        if let Some(ref init_expr) = decl.initial_value {
            let init_val = self.emit_expression(init_expr)?;
            let coerced = self.coerce_value(init_val, &rt);
            self.builder.build_store(ptr, coerced).unwrap();
        } else {
            // Zero-initialise
            let zero = self.zero_value(&rt);
            self.builder.build_store(ptr, zero).unwrap();
        }

        self.variables.insert(
            decl.name.clone(),
            VarSlot {
                ptr,
                resolved_type: rt,
                llvm_type: lt,
            },
        );
        Ok(())
    }

    /// Returns the zero/default value for a resolved type.
    fn zero_value(&self, rt: &ResolvedType) -> BasicValueEnum<'ctx> {
        match rt {
            ResolvedType::Bool => self.context.bool_type().const_zero().into(),
            ResolvedType::SignedInt { bits }
            | ResolvedType::UnsignedInt { bits }
            | ResolvedType::BitString { bits } => self
                .context
                .custom_width_int_type(*bits)
                .const_zero()
                .into(),
            ResolvedType::Float { bits: 32 } => self.context.f32_type().const_zero().into(),
            ResolvedType::Float { bits: 64 } | ResolvedType::Float { .. } => {
                self.context.f64_type().const_zero().into()
            }
            ResolvedType::Time | ResolvedType::Date | ResolvedType::Tod | ResolvedType::Dt => {
                self.context.i64_type().const_zero().into()
            }
            ResolvedType::Array { .. }
            | ResolvedType::Str { .. }
            | ResolvedType::WStr { .. }
            | ResolvedType::UserDefined { .. } => {
                let ty = self.llvm_type(rt);
                ty.const_zero()
            }
            _ => self.context.i32_type().const_zero().into(),
        }
    }

    // ── Statement emission ──────────────────────────────────────

    fn emit_statements(&mut self, stmts: &[Statement]) -> CgResult<()> {
        for stmt in stmts {
            self.emit_statement(stmt)?;
            // If block was terminated (e.g. by RETURN/EXIT), stop
            if !self.needs_terminator() {
                break;
            }
        }
        Ok(())
    }

    fn emit_statement(&mut self, stmt: &Statement) -> CgResult<()> {
        match stmt {
            Statement::Assignment { target, value, .. } => self.emit_assignment(target, value),
            Statement::If {
                condition,
                then_body,
                elsif_branches,
                else_body,
                ..
            } => self.emit_if(condition, then_body, elsif_branches, else_body),
            Statement::For {
                variable,
                from,
                to,
                by,
                body,
                ..
            } => self.emit_for(variable, from, to, by, body),
            Statement::While {
                condition, body, ..
            } => self.emit_while(condition, body),
            Statement::Repeat {
                body, condition, ..
            } => self.emit_repeat(body, condition),
            Statement::Case {
                selector,
                branches,
                else_body,
                ..
            } => self.emit_case(selector, branches, else_body),
            Statement::Exit { .. } => {
                if let Some(exit_bb) = self.loop_exit_stack.last() {
                    self.builder.build_unconditional_branch(*exit_bb).unwrap();
                }
                Ok(())
            }
            Statement::Return { .. } => {
                // For functions: load return value and return it
                // For programs: just return void
                let function = self.current_fn.unwrap();
                let ret_type = function.get_type().get_return_type();
                if let Some(_) = ret_type {
                    // Function with return type — return value is in a
                    // variable named after the function
                    let fn_name = function.get_name().to_str().unwrap_or("");
                    if let Some(slot) = self.variables.get(fn_name) {
                        let val = self
                            .builder
                            .build_load(slot.llvm_type, slot.ptr, "retval")
                            .unwrap();
                        self.builder.build_return(Some(&val)).unwrap();
                    } else {
                        self.builder.build_return(None).unwrap();
                    }
                } else {
                    self.builder.build_return(None).unwrap();
                }
                Ok(())
            }
            Statement::CallStatement { name, args, .. } => {
                self.emit_call(name, args)?;
                Ok(())
            }
            Statement::Empty => Ok(()),
        }
    }

    // ── Assignment ──────────────────────────────────────────────

    fn emit_assignment(&mut self, target: &Expression, value: &Expression) -> CgResult<()> {
        let rhs = self.emit_expression(value)?;

        match target {
            Expression::Identifier { name, .. } => {
                let slot = self
                    .variables
                    .get(name)
                    .cloned()
                    .ok_or_else(|| CodegenError {
                        message: format!("undefined variable '{}'", name),
                    })?;
                let coerced = self.coerce_value(rhs, &slot.resolved_type);
                self.builder.build_store(slot.ptr, coerced).unwrap();
                Ok(())
            }
            Expression::ArrayAccess { array, indices, .. } => {
                let ptr = self.emit_array_gep(array, indices)?;
                self.builder.build_store(ptr, rhs).unwrap();
                Ok(())
            }
            Expression::MemberAccess { object, member, .. } => {
                let (ptr, field_type) = self.emit_member_gep(object, member)?;
                let coerced = self.coerce_value(rhs, &field_type);
                self.builder.build_store(ptr, coerced).unwrap();
                Ok(())
            }
            _ => Err(CodegenError {
                message: "unsupported assignment target".to_string(),
            }),
        }
    }

    // ── Function block member access ────────────────────────────

    /// Computes a pointer to `instance.member`, returning it with the
    /// field's resolved type.
    fn emit_member_gep(
        &mut self,
        object: &Expression,
        member: &str,
    ) -> CgResult<(PointerValue<'ctx>, ResolvedType)> {
        let Expression::Identifier { name, .. } = object else {
            return Err(CodegenError {
                message: "only function block instances support member access".to_string(),
            });
        };

        let slot = self
            .variables
            .get(name)
            .cloned()
            .ok_or_else(|| CodegenError {
                message: format!("undefined variable '{}'", name),
            })?;

        let (fb_name, layout) = self.instance_layout(&slot).ok_or_else(|| CodegenError {
            message: format!("'{}' is not a function block instance", name),
        })?;

        let field = layout.field(member).ok_or_else(|| CodegenError {
            message: format!("function block {} has no member '{}'", fb_name, member),
        })?;

        let ptr = self
            .builder
            .build_struct_gep(
                layout.struct_type,
                slot.ptr,
                field.index,
                &format!("{}.{}", name, field.name),
            )
            .map_err(|e| CodegenError {
                message: format!("cannot address '{}.{}': {}", name, member, e),
            })?;

        Ok((ptr, field.resolved_type.clone()))
    }

    // ── IF statement ────────────────────────────────────────────

    fn emit_if(
        &mut self,
        condition: &Expression,
        then_body: &[Statement],
        elsif_branches: &[(Expression, Vec<Statement>)],
        else_body: &Option<Vec<Statement>>,
    ) -> CgResult<()> {
        let function = self.current_fn.unwrap();
        let merge_bb = self.context.append_basic_block(function, "if.merge");

        // Main IF branch
        let then_bb = self.context.append_basic_block(function, "if.then");
        let next_bb = if !elsif_branches.is_empty() || else_body.is_some() {
            self.context.append_basic_block(function, "if.else")
        } else {
            merge_bb
        };

        let cond_val = self.emit_expression(condition)?;
        let cond_bool = self.to_bool(cond_val);
        self.builder
            .build_conditional_branch(cond_bool, then_bb, next_bb)
            .unwrap();

        // Then block
        self.builder.position_at_end(then_bb);
        self.emit_statements(then_body)?;
        if self.needs_terminator() {
            self.builder.build_unconditional_branch(merge_bb).unwrap();
        }

        // ELSIF chains
        let mut current_else_bb = next_bb;
        for (i, (cond, body)) in elsif_branches.iter().enumerate() {
            self.builder.position_at_end(current_else_bb);

            let elsif_then = self
                .context
                .append_basic_block(function, &format!("elsif.then.{}", i));
            let elsif_next = if i + 1 < elsif_branches.len() || else_body.is_some() {
                self.context
                    .append_basic_block(function, &format!("elsif.else.{}", i))
            } else {
                merge_bb
            };

            let cval = self.emit_expression(cond)?;
            let cbool = self.to_bool(cval);
            self.builder
                .build_conditional_branch(cbool, elsif_then, elsif_next)
                .unwrap();

            self.builder.position_at_end(elsif_then);
            self.emit_statements(body)?;
            if self.needs_terminator() {
                self.builder.build_unconditional_branch(merge_bb).unwrap();
            }

            current_else_bb = elsif_next;
        }

        // ELSE block
        if let Some(body) = else_body {
            self.builder.position_at_end(current_else_bb);
            self.emit_statements(body)?;
            if self.needs_terminator() {
                self.builder.build_unconditional_branch(merge_bb).unwrap();
            }
        }

        self.builder.position_at_end(merge_bb);
        Ok(())
    }

    // ── FOR loop ────────────────────────────────────────────────

    fn emit_for(
        &mut self,
        variable: &str,
        from: &Expression,
        to: &Expression,
        by: &Option<Expression>,
        body: &[Statement],
    ) -> CgResult<()> {
        let function = self.current_fn.unwrap();
        let slot = self
            .variables
            .get(variable)
            .cloned()
            .ok_or_else(|| CodegenError {
                message: format!("undefined FOR variable '{}'", variable),
            })?;

        // Evaluate bounds
        let from_val = self.emit_expression(from)?;
        let to_val = self.emit_expression(to)?;
        let step_val = if let Some(by_expr) = by {
            self.emit_expression(by_expr)?
        } else {
            // Default step = 1
            let bits = slot.resolved_type.int_bits().unwrap_or(32);
            self.context
                .custom_width_int_type(bits)
                .const_int(1, false)
                .into()
        };

        // Coerce to loop variable type
        let from_coerced = self.coerce_value(from_val, &slot.resolved_type);
        let to_coerced = self.coerce_value(to_val, &slot.resolved_type);

        // Store initial value
        self.builder.build_store(slot.ptr, from_coerced).unwrap();

        // Create blocks
        let cond_bb = self.context.append_basic_block(function, "for.cond");
        let body_bb = self.context.append_basic_block(function, "for.body");
        let inc_bb = self.context.append_basic_block(function, "for.inc");
        let exit_bb = self.context.append_basic_block(function, "for.exit");

        self.builder.build_unconditional_branch(cond_bb).unwrap();

        // Condition: i <= to
        self.builder.position_at_end(cond_bb);
        let current = self
            .builder
            .build_load(slot.llvm_type, slot.ptr, "for.cur")
            .unwrap();
        let cmp = if slot.resolved_type.is_signed() {
            self.builder
                .build_int_compare(
                    IntPredicate::SLE,
                    current.into_int_value(),
                    to_coerced.into_int_value(),
                    "for.cmp",
                )
                .unwrap()
        } else {
            self.builder
                .build_int_compare(
                    IntPredicate::ULE,
                    current.into_int_value(),
                    to_coerced.into_int_value(),
                    "for.cmp",
                )
                .unwrap()
        };
        self.builder
            .build_conditional_branch(cmp, body_bb, exit_bb)
            .unwrap();

        // Body
        self.builder.position_at_end(body_bb);
        self.loop_exit_stack.push(exit_bb);
        self.emit_statements(body)?;
        self.loop_exit_stack.pop();
        if self.needs_terminator() {
            self.builder.build_unconditional_branch(inc_bb).unwrap();
        }

        // Increment: i := i + step
        self.builder.position_at_end(inc_bb);
        let cur = self
            .builder
            .build_load(slot.llvm_type, slot.ptr, "for.cur2")
            .unwrap();
        let step_coerced = self.coerce_value(step_val, &slot.resolved_type);
        let next = self
            .builder
            .build_int_add(
                cur.into_int_value(),
                step_coerced.into_int_value(),
                "for.next",
            )
            .unwrap();
        self.builder.build_store(slot.ptr, next).unwrap();
        self.builder.build_unconditional_branch(cond_bb).unwrap();

        // Exit
        self.builder.position_at_end(exit_bb);
        Ok(())
    }

    // ── WHILE loop ──────────────────────────────────────────────

    fn emit_while(&mut self, condition: &Expression, body: &[Statement]) -> CgResult<()> {
        let function = self.current_fn.unwrap();
        let cond_bb = self.context.append_basic_block(function, "while.cond");
        let body_bb = self.context.append_basic_block(function, "while.body");
        let exit_bb = self.context.append_basic_block(function, "while.exit");

        self.builder.build_unconditional_branch(cond_bb).unwrap();

        self.builder.position_at_end(cond_bb);
        let cond_val = self.emit_expression(condition)?;
        let cond_bool = self.to_bool(cond_val);
        self.builder
            .build_conditional_branch(cond_bool, body_bb, exit_bb)
            .unwrap();

        self.builder.position_at_end(body_bb);
        self.loop_exit_stack.push(exit_bb);
        self.emit_statements(body)?;
        self.loop_exit_stack.pop();
        if self.needs_terminator() {
            self.builder.build_unconditional_branch(cond_bb).unwrap();
        }

        self.builder.position_at_end(exit_bb);
        Ok(())
    }

    // ── REPEAT loop ─────────────────────────────────────────────

    fn emit_repeat(&mut self, body: &[Statement], condition: &Expression) -> CgResult<()> {
        let function = self.current_fn.unwrap();
        let body_bb = self.context.append_basic_block(function, "repeat.body");
        let cond_bb = self.context.append_basic_block(function, "repeat.cond");
        let exit_bb = self.context.append_basic_block(function, "repeat.exit");

        self.builder.build_unconditional_branch(body_bb).unwrap();

        self.builder.position_at_end(body_bb);
        self.loop_exit_stack.push(exit_bb);
        self.emit_statements(body)?;
        self.loop_exit_stack.pop();
        if self.needs_terminator() {
            self.builder.build_unconditional_branch(cond_bb).unwrap();
        }

        // UNTIL: exit when condition is TRUE
        self.builder.position_at_end(cond_bb);
        let cond_val = self.emit_expression(condition)?;
        let cond_bool = self.to_bool(cond_val);
        self.builder
            .build_conditional_branch(cond_bool, exit_bb, body_bb)
            .unwrap();

        self.builder.position_at_end(exit_bb);
        Ok(())
    }

    // ── CASE statement ──────────────────────────────────────────

    fn emit_case(
        &mut self,
        selector: &Expression,
        branches: &[CaseBranch],
        else_body: &Option<Vec<Statement>>,
    ) -> CgResult<()> {
        let function = self.current_fn.unwrap();
        let sel_val = self.emit_expression(selector)?.into_int_value();
        let merge_bb = self.context.append_basic_block(function, "case.merge");

        let default_bb = if else_body.is_some() {
            self.context.append_basic_block(function, "case.else")
        } else {
            merge_bb
        };

        // Build a series of comparisons and branches
        let mut next_test_bb = self.context.append_basic_block(function, "case.test.0");
        self.builder
            .build_unconditional_branch(next_test_bb)
            .unwrap();

        for (i, branch) in branches.iter().enumerate() {
            self.builder.position_at_end(next_test_bb);

            let body_bb = self
                .context
                .append_basic_block(function, &format!("case.body.{}", i));
            let following_test = if i + 1 < branches.len() {
                self.context
                    .append_basic_block(function, &format!("case.test.{}", i + 1))
            } else {
                default_bb
            };

            // Test all labels for this branch (OR logic)
            let mut match_val = self.context.bool_type().const_int(0, false);
            for label in &branch.labels {
                let label_match = match label {
                    CaseLabel::Value(expr) => {
                        let v = self.emit_expression(expr)?.into_int_value();
                        self.builder
                            .build_int_compare(IntPredicate::EQ, sel_val, v, "case.eq")
                            .unwrap()
                    }
                    CaseLabel::Range(lo_expr, hi_expr) => {
                        let lo = self.emit_expression(lo_expr)?.into_int_value();
                        let hi = self.emit_expression(hi_expr)?.into_int_value();
                        let ge = self
                            .builder
                            .build_int_compare(IntPredicate::SGE, sel_val, lo, "case.ge")
                            .unwrap();
                        let le = self
                            .builder
                            .build_int_compare(IntPredicate::SLE, sel_val, hi, "case.le")
                            .unwrap();
                        self.builder.build_and(ge, le, "case.inrange").unwrap()
                    }
                };
                match_val = self
                    .builder
                    .build_or(match_val, label_match, "case.or")
                    .unwrap();
            }

            self.builder
                .build_conditional_branch(match_val, body_bb, following_test)
                .unwrap();

            // Emit branch body
            self.builder.position_at_end(body_bb);
            self.emit_statements(&branch.body)?;
            if self.needs_terminator() {
                self.builder.build_unconditional_branch(merge_bb).unwrap();
            }

            next_test_bb = following_test;
        }

        // ELSE block
        if let Some(body) = else_body {
            self.builder.position_at_end(default_bb);
            self.emit_statements(body)?;
            if self.needs_terminator() {
                self.builder.build_unconditional_branch(merge_bb).unwrap();
            }
        }

        self.builder.position_at_end(merge_bb);
        Ok(())
    }

    // ── Expression emission ─────────────────────────────────────

    /// Emits LLVM IR for an expression, returning the SSA value.
    fn emit_expression(&mut self, expr: &Expression) -> CgResult<BasicValueEnum<'ctx>> {
        match expr {
            Expression::IntLiteral { value, .. } => {
                // Default integer literal is i32 (DINT)
                Ok(self
                    .context
                    .i32_type()
                    .const_int(*value as u64, true)
                    .into())
            }
            Expression::RealLiteral { value, .. } => {
                Ok(self.context.f64_type().const_float(*value).into())
            }
            Expression::BoolLiteral { value, .. } => Ok(self
                .context
                .bool_type()
                .const_int(*value as u64, false)
                .into()),
            Expression::StringLiteral { .. } | Expression::WStringLiteral { .. } => {
                // String literals as global constants — simplified for now
                Ok(self.context.i32_type().const_zero().into())
            }
            Expression::TimeLiteral { text, .. } => {
                // TIME is an i64 millisecond count — see `stdlib`.
                let ms = stdlib::parse_time_literal(text).ok_or_else(|| CodegenError {
                    message: format!("malformed duration literal '{}'", text),
                })?;
                Ok(self.context.i64_type().const_int(ms as u64, true).into())
            }

            Expression::DateLiteral { .. }
            | Expression::TodLiteral { .. }
            | Expression::DtLiteral { .. } => {
                // Calendar literals — placeholder as i64 zero
                Ok(self.context.i64_type().const_zero().into())
            }

            Expression::Identifier { name, .. } => {
                let slot = self
                    .variables
                    .get(name)
                    .cloned()
                    .ok_or_else(|| CodegenError {
                        message: format!("undefined variable '{}'", name),
                    })?;
                let val = self
                    .builder
                    .build_load(slot.llvm_type, slot.ptr, name)
                    .unwrap();
                Ok(val)
            }

            Expression::BinaryOp {
                left, op, right, ..
            } => {
                let lhs = self.emit_expression(left)?;
                let rhs = self.emit_expression(right)?;
                self.emit_binary_op(lhs, *op, rhs)
            }

            Expression::UnaryOp { op, operand, .. } => {
                let val = self.emit_expression(operand)?;
                self.emit_unary_op(*op, val)
            }

            Expression::FunctionCall { name, args, .. } => self.emit_call(name, args),

            Expression::ArrayAccess { array, indices, .. } => {
                let ptr = self.emit_array_gep(array, indices)?;
                // Determine element type from the array type
                let elem_ty = self.get_array_element_type(array);
                let val = self.builder.build_load(elem_ty, ptr, "arr.load").unwrap();
                Ok(val)
            }

            Expression::MemberAccess { object, member, .. } => {
                let (ptr, field_type) = self.emit_member_gep(object, member)?;
                let llvm_type = self.llvm_type(&field_type);
                let val = self.builder.build_load(llvm_type, ptr, member).unwrap();
                Ok(val)
            }
        }
    }

    // ── Binary operators ────────────────────────────────────────

    fn emit_binary_op(
        &mut self,
        lhs: BasicValueEnum<'ctx>,
        op: BinaryOperator,
        rhs: BasicValueEnum<'ctx>,
    ) -> CgResult<BasicValueEnum<'ctx>> {
        // Determine if we're doing int or float ops
        let lhs_is_float = lhs.is_float_value();
        let rhs_is_float = rhs.is_float_value();

        if lhs_is_float || rhs_is_float {
            // Float operations
            let l = self.to_f64(lhs);
            let r = self.to_f64(rhs);
            self.emit_float_binary_op(l, op, r)
        } else {
            // Integer / boolean operations
            let l = lhs.into_int_value();
            let r = self.coerce_int(rhs.into_int_value(), l.get_type().get_bit_width());
            self.emit_int_binary_op(l, op, r)
        }
    }

    fn emit_int_binary_op(
        &self,
        lhs: IntValue<'ctx>,
        op: BinaryOperator,
        rhs: IntValue<'ctx>,
    ) -> CgResult<BasicValueEnum<'ctx>> {
        let result: BasicValueEnum = match op {
            BinaryOperator::Add => self.builder.build_int_add(lhs, rhs, "add").unwrap().into(),
            BinaryOperator::Sub => self.builder.build_int_sub(lhs, rhs, "sub").unwrap().into(),
            BinaryOperator::Mul => self.builder.build_int_mul(lhs, rhs, "mul").unwrap().into(),
            BinaryOperator::Div => self
                .builder
                .build_int_signed_div(lhs, rhs, "div")
                .unwrap()
                .into(),
            BinaryOperator::Mod => self
                .builder
                .build_int_signed_rem(lhs, rhs, "mod")
                .unwrap()
                .into(),
            BinaryOperator::Power => {
                // Integer power — convert to float, use intrinsic, convert back
                let lf = self
                    .builder
                    .build_signed_int_to_float(lhs, self.context.f64_type(), "pow.l")
                    .unwrap();
                let rf = self
                    .builder
                    .build_signed_int_to_float(rhs, self.context.f64_type(), "pow.r")
                    .unwrap();
                let result = self.emit_float_pow(lf, rf);
                self.builder
                    .build_float_to_signed_int(result, lhs.get_type(), "pow.i")
                    .unwrap()
                    .into()
            }
            BinaryOperator::Eq => self
                .builder
                .build_int_compare(IntPredicate::EQ, lhs, rhs, "eq")
                .unwrap()
                .into(),
            BinaryOperator::Neq => self
                .builder
                .build_int_compare(IntPredicate::NE, lhs, rhs, "ne")
                .unwrap()
                .into(),
            BinaryOperator::Lt => self
                .builder
                .build_int_compare(IntPredicate::SLT, lhs, rhs, "lt")
                .unwrap()
                .into(),
            BinaryOperator::Le => self
                .builder
                .build_int_compare(IntPredicate::SLE, lhs, rhs, "le")
                .unwrap()
                .into(),
            BinaryOperator::Gt => self
                .builder
                .build_int_compare(IntPredicate::SGT, lhs, rhs, "gt")
                .unwrap()
                .into(),
            BinaryOperator::Ge => self
                .builder
                .build_int_compare(IntPredicate::SGE, lhs, rhs, "ge")
                .unwrap()
                .into(),
            BinaryOperator::And => self.builder.build_and(lhs, rhs, "and").unwrap().into(),
            BinaryOperator::Or => self.builder.build_or(lhs, rhs, "or").unwrap().into(),
            BinaryOperator::Xor => self.builder.build_xor(lhs, rhs, "xor").unwrap().into(),
        };
        Ok(result)
    }

    fn emit_float_binary_op(
        &self,
        lhs: FloatValue<'ctx>,
        op: BinaryOperator,
        rhs: FloatValue<'ctx>,
    ) -> CgResult<BasicValueEnum<'ctx>> {
        let result: BasicValueEnum = match op {
            BinaryOperator::Add => self
                .builder
                .build_float_add(lhs, rhs, "fadd")
                .unwrap()
                .into(),
            BinaryOperator::Sub => self
                .builder
                .build_float_sub(lhs, rhs, "fsub")
                .unwrap()
                .into(),
            BinaryOperator::Mul => self
                .builder
                .build_float_mul(lhs, rhs, "fmul")
                .unwrap()
                .into(),
            BinaryOperator::Div => self
                .builder
                .build_float_div(lhs, rhs, "fdiv")
                .unwrap()
                .into(),
            BinaryOperator::Power => self.emit_float_pow(lhs, rhs).into(),
            BinaryOperator::Eq => self
                .builder
                .build_float_compare(FloatPredicate::OEQ, lhs, rhs, "feq")
                .unwrap()
                .into(),
            BinaryOperator::Neq => self
                .builder
                .build_float_compare(FloatPredicate::ONE, lhs, rhs, "fne")
                .unwrap()
                .into(),
            BinaryOperator::Lt => self
                .builder
                .build_float_compare(FloatPredicate::OLT, lhs, rhs, "flt")
                .unwrap()
                .into(),
            BinaryOperator::Le => self
                .builder
                .build_float_compare(FloatPredicate::OLE, lhs, rhs, "fle")
                .unwrap()
                .into(),
            BinaryOperator::Gt => self
                .builder
                .build_float_compare(FloatPredicate::OGT, lhs, rhs, "fgt")
                .unwrap()
                .into(),
            BinaryOperator::Ge => self
                .builder
                .build_float_compare(FloatPredicate::OGE, lhs, rhs, "fge")
                .unwrap()
                .into(),
            BinaryOperator::Mod => self
                .builder
                .build_float_rem(lhs, rhs, "frem")
                .unwrap()
                .into(),
            // Boolean ops on float — shouldn't happen after semantic analysis
            _ => self.context.f64_type().const_zero().into(),
        };
        Ok(result)
    }

    /// Emits `llvm.pow.f64` intrinsic.
    fn emit_float_pow(&self, base: FloatValue<'ctx>, exp: FloatValue<'ctx>) -> FloatValue<'ctx> {
        let f64_type = self.context.f64_type();
        let pow_type = f64_type.fn_type(&[f64_type.into(), f64_type.into()], false);
        let pow_fn = self
            .module
            .get_function("llvm.pow.f64")
            .unwrap_or_else(|| self.module.add_function("llvm.pow.f64", pow_type, None));
        let result = self
            .builder
            .build_call(pow_fn, &[base.into(), exp.into()], "pow")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap();
        result.into_float_value()
    }

    // ── Unary operators ─────────────────────────────────────────

    fn emit_unary_op(
        &self,
        op: UnaryOperator,
        val: BasicValueEnum<'ctx>,
    ) -> CgResult<BasicValueEnum<'ctx>> {
        match op {
            UnaryOperator::Neg => {
                if val.is_float_value() {
                    Ok(self
                        .builder
                        .build_float_neg(val.into_float_value(), "fneg")
                        .unwrap()
                        .into())
                } else {
                    Ok(self
                        .builder
                        .build_int_neg(val.into_int_value(), "neg")
                        .unwrap()
                        .into())
                }
            }
            UnaryOperator::Pos => Ok(val), // no-op
            UnaryOperator::Not => Ok(self
                .builder
                .build_not(val.into_int_value(), "not")
                .unwrap()
                .into()),
        }
    }

    // ── Function calls ──────────────────────────────────────────

    fn emit_call(&mut self, name: &str, args: &[CallArg]) -> CgResult<BasicValueEnum<'ctx>> {
        // The scan-clock intrinsic reads the runtime's millisecond
        // counter; it has no ST body to call.
        if name.eq_ignore_ascii_case(stdlib::TIME_MS_INTRINSIC) {
            return self.emit_time_ms();
        }

        // `inst(IN := x, Q => y);` — the callee names a variable holding
        // a function block instance, not a function.
        if let Some(slot) = self.variables.get(name).cloned() {
            if let Some((fb_name, layout)) = self.instance_layout(&slot) {
                return self.emit_fb_invoke(name, &fb_name, &layout, slot.ptr, args);
            }
        }

        let function = self
            .functions
            .get(name)
            .cloned()
            .ok_or_else(|| CodegenError {
                message: format!("undefined function '{}'", name),
            })?;

        // Evaluate arguments
        let mut arg_vals: Vec<BasicValueEnum<'ctx>> = Vec::new();
        for arg in args {
            match arg {
                CallArg::Positional(expr) | CallArg::Named { value: expr, .. } => {
                    arg_vals.push(self.emit_expression(expr)?);
                }
                CallArg::Output { .. } => {
                    // Output args handled after the call
                }
            }
        }

        let args_meta: Vec<inkwell::values::BasicMetadataValueEnum> =
            arg_vals.iter().map(|v| (*v).into()).collect();

        let result = self
            .builder
            .build_call(function, &args_meta, "call")
            .unwrap()
            .try_as_basic_value();

        match result.left() {
            Some(val) => Ok(val),
            None => Ok(self.context.i32_type().const_zero().into()),
        }
    }

    // ── Function block invocation ───────────────────────────────

    /// Emits a call on a function block instance.
    ///
    /// Three steps, in the order IEC 61131-3 requires: copy the bound
    /// inputs into the instance struct, run the block body against that
    /// struct, then copy any `=>` outputs back out. Outputs read later
    /// as `inst.Q` need no copy-back — they are already in the struct.
    fn emit_fb_invoke(
        &mut self,
        instance: &str,
        fb_name: &str,
        layout: &FbLayout<'ctx>,
        instance_ptr: PointerValue<'ctx>,
        args: &[CallArg],
    ) -> CgResult<BasicValueEnum<'ctx>> {
        let inputs: Vec<FbField> = layout.inputs().cloned().collect();
        let mut positional = 0usize;

        for arg in args {
            let (field, value_expr) = match arg {
                CallArg::Positional(expr) => {
                    let field = inputs.get(positional).cloned().ok_or_else(|| CodegenError {
                        message: format!(
                            "'{}' ({}) takes {} input(s), but more were given",
                            instance,
                            fb_name,
                            inputs.len()
                        ),
                    })?;
                    positional += 1;
                    (field, expr)
                }
                CallArg::Named { name, value } => {
                    let field = layout.field(name).cloned().ok_or_else(|| CodegenError {
                        message: format!("function block {} has no input '{}'", fb_name, name),
                    })?;
                    (field, value)
                }
                // Outputs are copied back after the body runs.
                CallArg::Output { .. } => continue,
            };

            let value = self.emit_expression(value_expr)?;
            let coerced = self.coerce_value(value, &field.resolved_type);
            let ptr = self.fb_field_ptr(layout, instance_ptr, &field, instance)?;
            self.builder.build_store(ptr, coerced).unwrap();
        }

        let function = self
            .functions
            .get(fb_name)
            .copied()
            .ok_or_else(|| CodegenError {
                message: format!("function block '{}' was never declared", fb_name),
            })?;
        self.builder
            .build_call(function, &[instance_ptr.into()], "")
            .unwrap();

        for arg in args {
            let CallArg::Output { name, target } = arg else {
                continue;
            };
            let field = layout.field(name).cloned().ok_or_else(|| CodegenError {
                message: format!("function block {} has no output '{}'", fb_name, name),
            })?;
            let ptr = self.fb_field_ptr(layout, instance_ptr, &field, instance)?;
            let llvm_type = self.llvm_type(&field.resolved_type);
            let value = self.builder.build_load(llvm_type, ptr, &field.name).unwrap();
            self.emit_store_to_target(target, value)?;
        }

        // An FB call is a statement, not an expression — nothing to yield.
        Ok(self.context.bool_type().const_zero().into())
    }

    /// Computes a pointer to one field of a function block instance.
    fn fb_field_ptr(
        &self,
        layout: &FbLayout<'ctx>,
        instance_ptr: PointerValue<'ctx>,
        field: &FbField,
        instance: &str,
    ) -> CgResult<PointerValue<'ctx>> {
        self.builder
            .build_struct_gep(
                layout.struct_type,
                instance_ptr,
                field.index,
                &format!("{}.{}", instance, field.name),
            )
            .map_err(|e| CodegenError {
                message: format!("cannot address '{}.{}': {}", instance, field.name, e),
            })
    }

    /// Stores an already-evaluated value into an assignment target.
    fn emit_store_to_target(
        &mut self,
        target: &Expression,
        value: BasicValueEnum<'ctx>,
    ) -> CgResult<()> {
        match target {
            Expression::Identifier { name, .. } => {
                let slot = self
                    .variables
                    .get(name)
                    .cloned()
                    .ok_or_else(|| CodegenError {
                        message: format!("undefined variable '{}'", name),
                    })?;
                let coerced = self.coerce_value(value, &slot.resolved_type);
                self.builder.build_store(slot.ptr, coerced).unwrap();
                Ok(())
            }
            Expression::ArrayAccess { array, indices, .. } => {
                let ptr = self.emit_array_gep(array, indices)?;
                self.builder.build_store(ptr, value).unwrap();
                Ok(())
            }
            Expression::MemberAccess { object, member, .. } => {
                let (ptr, field_type) = self.emit_member_gep(object, member)?;
                let coerced = self.coerce_value(value, &field_type);
                self.builder.build_store(ptr, coerced).unwrap();
                Ok(())
            }
            _ => Err(CodegenError {
                message: "unsupported output assignment target".to_string(),
            }),
        }
    }

    // ── Scan clock intrinsic ────────────────────────────────────

    /// Emits `TIME_MS()` — a load of the runtime's scan clock.
    fn emit_time_ms(&mut self) -> CgResult<BasicValueEnum<'ctx>> {
        let global = self.ensure_time_support();
        let value = self
            .builder
            .build_load(self.context.i64_type(), global, "now_ms")
            .unwrap();
        Ok(value)
    }

    /// Creates the scan clock global and its host-facing setter, once
    /// per module, and returns a pointer to the global.
    ///
    /// Emitted lazily: a program with no timers gets neither symbol.
    fn ensure_time_support(&mut self) -> PointerValue<'ctx> {
        if let Some(global) = self.module.get_global(TIME_GLOBAL) {
            return global.as_pointer_value();
        }

        let i64_type = self.context.i64_type();
        let global = self.module.add_global(i64_type, None, TIME_GLOBAL);
        global.set_initializer(&i64_type.const_zero());

        // The setter gets its own block, so remember where the caller
        // was building and go back there afterwards.
        let resume = self.builder.get_insert_block();

        let setter_type = self
            .context
            .void_type()
            .fn_type(&[i64_type.into()], false);
        let setter = self.module.add_function(TIME_SETTER_FN, setter_type, None);
        let entry = self.context.append_basic_block(setter, "entry");
        self.builder.position_at_end(entry);
        let millis = setter
            .get_first_param()
            .expect("scan clock setter takes one parameter");
        self.builder
            .build_store(global.as_pointer_value(), millis)
            .unwrap();
        self.builder.build_return(None).unwrap();

        if let Some(block) = resume {
            self.builder.position_at_end(block);
        }

        global.as_pointer_value()
    }

    // ── Array access ────────────────────────────────────────────

    /// Computes a GEP (GetElementPtr) for an array subscript.
    fn emit_array_gep(
        &mut self,
        array: &Expression,
        indices: &[Expression],
    ) -> CgResult<PointerValue<'ctx>> {
        if let Expression::Identifier { name, .. } = array {
            let slot = self
                .variables
                .get(name)
                .cloned()
                .ok_or_else(|| CodegenError {
                    message: format!("undefined array '{}'", name),
                })?;

            // Get array range offset
            let low_offset = if let ResolvedType::Array { ranges, .. } = &slot.resolved_type {
                ranges.first().map(|r| r.low).unwrap_or(0)
            } else {
                0
            };

            let idx_val = self.emit_expression(&indices[0])?.into_int_value();
            // Subtract low bound if non-zero: effective_idx = idx - low
            let effective_idx = if low_offset != 0 {
                let offset = self.context.i32_type().const_int(low_offset as u64, true);
                let idx32 = self.coerce_int(idx_val, 32);
                self.builder
                    .build_int_sub(idx32, offset, "arr.idx")
                    .unwrap()
            } else {
                self.coerce_int(idx_val, 32)
            };

            let zero = self.context.i32_type().const_zero();
            let ptr = unsafe {
                self.builder
                    .build_gep(slot.llvm_type, slot.ptr, &[zero, effective_idx], "arr.gep")
                    .unwrap()
            };
            Ok(ptr)
        } else {
            Err(CodegenError {
                message: "complex array access not supported".to_string(),
            })
        }
    }

    /// Determines the LLVM element type of an array expression.
    fn get_array_element_type(&self, array: &Expression) -> BasicTypeEnum<'ctx> {
        if let Expression::Identifier { name, .. } = array {
            if let Some(slot) = self.variables.get(name) {
                if let ResolvedType::Array { element, .. } = &slot.resolved_type {
                    return self.llvm_type(element);
                }
            }
        }
        self.context.i32_type().into()
    }

    // ── Type coercion helpers ───────────────────────────────────

    /// Converts a value to bool (i1). Integers are compared != 0.
    fn to_bool(&self, val: BasicValueEnum<'ctx>) -> IntValue<'ctx> {
        if val.is_int_value() {
            let iv = val.into_int_value();
            if iv.get_type().get_bit_width() == 1 {
                iv
            } else {
                let zero = iv.get_type().const_zero();
                self.builder
                    .build_int_compare(IntPredicate::NE, iv, zero, "tobool")
                    .unwrap()
            }
        } else if val.is_float_value() {
            let fv = val.into_float_value();
            let zero = fv.get_type().const_zero();
            self.builder
                .build_float_compare(FloatPredicate::ONE, fv, zero, "ftobool")
                .unwrap()
        } else {
            self.context.bool_type().const_int(0, false)
        }
    }

    /// Converts any numeric value to f64.
    fn to_f64(&self, val: BasicValueEnum<'ctx>) -> FloatValue<'ctx> {
        if val.is_float_value() {
            let fv = val.into_float_value();
            if fv.get_type() == self.context.f64_type() {
                fv
            } else {
                self.builder
                    .build_float_ext(fv, self.context.f64_type(), "fext")
                    .unwrap()
            }
        } else {
            self.builder
                .build_signed_int_to_float(val.into_int_value(), self.context.f64_type(), "itof")
                .unwrap()
        }
    }

    /// Coerces an integer to a different bit width (sign-extend or truncate).
    fn coerce_int(&self, val: IntValue<'ctx>, target_bits: u32) -> IntValue<'ctx> {
        let src_bits = val.get_type().get_bit_width();
        if src_bits == target_bits {
            val
        } else if src_bits < target_bits {
            let target_ty = self.context.custom_width_int_type(target_bits);
            self.builder
                .build_int_s_extend(val, target_ty, "sext")
                .unwrap()
        } else {
            let target_ty = self.context.custom_width_int_type(target_bits);
            self.builder
                .build_int_truncate(val, target_ty, "trunc")
                .unwrap()
        }
    }

    /// Coerces a value to match a target resolved type.
    fn coerce_value(
        &self,
        val: BasicValueEnum<'ctx>,
        target: &ResolvedType,
    ) -> BasicValueEnum<'ctx> {
        let target_llvm = self.llvm_type(target);

        if val.is_float_value() && target.is_float() {
            let fv = val.into_float_value();
            if let BasicTypeEnum::FloatType(ft) = target_llvm {
                if fv.get_type() == ft {
                    return val;
                }
                // If it is f32 (and doesn't match the target), we must be extending to f64
                if fv.get_type() == self.context.f32_type() {
                    return self.builder.build_float_ext(fv, ft, "fext").unwrap().into();
                } else {
                    // Otherwise, we are truncating from f64 down to f32
                    return self
                        .builder
                        .build_float_trunc(fv, ft, "ftrunc")
                        .unwrap()
                        .into();
                }
            }
        }

        if val.is_int_value() && target.is_float() {
            if let BasicTypeEnum::FloatType(ft) = target_llvm {
                return self
                    .builder
                    .build_signed_int_to_float(val.into_int_value(), ft, "itof")
                    .unwrap()
                    .into();
            }
        }

        if val.is_float_value() && target.is_integer() {
            if let BasicTypeEnum::IntType(it) = target_llvm {
                return self
                    .builder
                    .build_float_to_signed_int(val.into_float_value(), it, "ftoi")
                    .unwrap()
                    .into();
            }
        }

        // Temporal types are i64 counts, so an integer flows into them
        // the same way it flows into any other integer slot.
        if val.is_int_value()
            && (target.is_integer() || target.is_temporal() || *target == ResolvedType::Bool)
        {
            let iv = val.into_int_value();
            if let BasicTypeEnum::IntType(it) = target_llvm {
                if iv.get_type().get_bit_width() != it.get_bit_width() {
                    return self.coerce_int(iv, it.get_bit_width()).into();
                }
            }
        }

        val
    }

    // ── Runtime compilation ─────────────────────────────────────
    //
    // These methods compile a PROGRAM for JIT execution in a scan
    // cycle loop. Variables become LLVM module-level globals that
    // persist across calls. Three functions are emitted per PROGRAM:
    //
    //   @__init_<Name>()        — stores initial values (call once)
    //   @__scan_<Name>()        — executes one scan cycle (call in loop)
    //   @__get_<var>() -> f64   — reads a variable as f64 (call to display)

    /// Compiles a compilation unit for runtime JIT execution.
    ///
    /// Unlike [`compile`](Self::compile) which uses stack allocas,
    /// this method stores PROGRAM variables as module-level globals
    /// so they persist across repeated scan cycle calls.
    ///
    /// Returns a list of [`RuntimeVar`] describing the exposed
    /// variables, which the runtime uses to call the getter functions
    /// and display values.
    pub fn compile_for_runtime(&mut self, unit: &CompilationUnit) -> CgResult<Vec<RuntimeVar>> {
        // Standard function blocks the program instantiates are compiled
        // alongside it — see `stdlib::inject`.
        let unit = stdlib::inject(unit);
        let mut runtime_vars = Vec::new();

        // Pass 0: name FB instance structs before laying any of them out.
        self.predeclare_fb_types(&unit);

        // Pass 1: Declare FUNCTION and FUNCTION_BLOCK signatures so they
        // can be called, and so instance layouts are known before the
        // PROGRAM allocates its globals.
        for pou in &unit.units {
            if matches!(pou, Pou::Function(_) | Pou::FunctionBlock(_)) {
                self.declare_pou(pou)?;
            }
        }

        // Pass 2: Compile each PROGRAM for runtime execution
        for pou in &unit.units {
            if let Pou::Program(p) = pou {
                let vars = self.compile_program_for_runtime(p)?;
                runtime_vars.extend(vars);
            }
        }

        // Pass 3: Emit FUNCTION bodies (they use normal allocas —
        // stateless) and FUNCTION_BLOCK bodies (state is the caller's).
        for pou in &unit.units {
            if matches!(pou, Pou::Function(_) | Pou::FunctionBlock(_)) {
                self.emit_pou(pou)?;
            }
        }

        Ok(runtime_vars)
    }

    /// Compiles a single PROGRAM for runtime execution.
    fn compile_program_for_runtime(&mut self, p: &ProgramDecl) -> CgResult<Vec<RuntimeVar>> {
        self.variables.clear();
        // Each exposed variable is carried with the storage its accessors
        // will address, so the two can never drift apart.
        let mut exposed: Vec<(RuntimeVar, RuntimeTarget<'ctx>)> = Vec::new();
        // Function block instances to initialise from __init_<Name>().
        let mut instances: Vec<(String, PointerValue<'ctx>)> = Vec::new();

        // ── Step 1: Create LLVM globals for each variable ──
        for block in &p.var_blocks {
            for decl in &block.declarations {
                let rt = Self::resolve_type(&decl.type_spec);
                let lt = self.llvm_type(&rt);
                let global_name = format!("__var_{}_{}", p.name, decl.name);

                let global = self.module.add_global(lt, None, &global_name);
                global.set_initializer(&lt.const_zero());

                let slot = VarSlot {
                    ptr: global.as_pointer_value(),
                    resolved_type: rt.clone(),
                    llvm_type: lt,
                };
                self.variables.insert(decl.name.clone(), slot.clone());

                if Self::is_runtime_scalar(&rt) {
                    exposed.push((
                        RuntimeVar {
                            name: decl.name.clone(),
                            resolved_type: rt.clone(),
                            program_name: p.name.clone(),
                            getter_fn_name: Self::runtime_accessor_name("get", &p.name, &decl.name),
                            setter_fn_name: Some(Self::runtime_accessor_name(
                                "set", &p.name, &decl.name,
                            )),
                        },
                        RuntimeTarget {
                            base: slot.ptr,
                            member: None,
                            resolved_type: rt,
                            llvm_type: lt,
                        },
                    ));
                } else if let Some((fb_name, layout)) = self.instance_layout(&slot) {
                    instances.push((fb_name, slot.ptr));

                    // Expose the block's outputs — `t.Q`, `t.ET` — so the
                    // dashboard and OPC UA server can watch timers and
                    // counters. They are read-only: the scan cycle owns them.
                    for field in layout.outputs() {
                        if !Self::is_runtime_scalar(&field.resolved_type) {
                            continue;
                        }
                        let accessor = format!("{}_{}", decl.name, field.name);
                        exposed.push((
                            RuntimeVar {
                                name: format!("{}.{}", decl.name, field.name),
                                resolved_type: field.resolved_type.clone(),
                                program_name: p.name.clone(),
                                getter_fn_name: Self::runtime_accessor_name(
                                    "get", &p.name, &accessor,
                                ),
                                setter_fn_name: None,
                            },
                            RuntimeTarget {
                                base: slot.ptr,
                                member: Some((layout.struct_type, field.index)),
                                resolved_type: field.resolved_type.clone(),
                                llvm_type: self.llvm_type(&field.resolved_type),
                            },
                        ));
                    }
                }
            }
        }

        // ── Step 2: Emit __init_<Name>() — stores initial values ──
        let init_name = format!("__init_{}", p.name);
        let void_fn_ty = self.context.void_type().fn_type(&[], false);
        let init_fn = self.module.add_function(&init_name, void_fn_ty, None);
        let entry = self.context.append_basic_block(init_fn, "entry");
        self.builder.position_at_end(entry);
        self.current_fn = Some(init_fn);

        for (fb_name, instance_ptr) in &instances {
            self.emit_fb_init_call(fb_name, *instance_ptr);
        }

        for block in &p.var_blocks {
            for decl in &block.declarations {
                if let Some(ref init_expr) = decl.initial_value {
                    let val = self.emit_expression(init_expr)?;
                    let slot = self.variables[&decl.name].clone();
                    let coerced = self.coerce_value(val, &slot.resolved_type);
                    self.builder.build_store(slot.ptr, coerced).unwrap();
                }
            }
        }
        self.builder.build_return(None).unwrap();

        // ── Step 3: Emit __scan_<Name>() — one scan cycle ──
        let scan_name = format!("__scan_{}", p.name);
        let scan_fn = self.module.add_function(&scan_name, void_fn_ty, None);
        let entry = self.context.append_basic_block(scan_fn, "entry");
        self.builder.position_at_end(entry);
        self.current_fn = Some(scan_fn);

        self.emit_statements(&p.body)?;
        if self.needs_terminator() {
            self.builder.build_return(None).unwrap();
        }

        // ── Step 4: Emit getter functions ──
        //
        // Each getter loads the global and converts to f64 so the
        // runtime can read all variables with one uniform type.
        for (var, target) in &exposed {
            self.emit_runtime_getter(target, &var.getter_fn_name)?;
            if let Some(setter_fn_name) = &var.setter_fn_name {
                self.emit_runtime_setter(target, setter_fn_name)?;
            }
        }

        self.current_fn = None;
        Ok(exposed.into_iter().map(|(var, _)| var).collect())
    }

    fn runtime_accessor_name(kind: &str, program_name: &str, variable_name: &str) -> String {
        format!("__{}_{}_{}", kind, program_name, variable_name)
    }

    fn is_runtime_scalar(rt: &ResolvedType) -> bool {
        matches!(
            rt,
            ResolvedType::Bool
                | ResolvedType::SignedInt { .. }
                | ResolvedType::UnsignedInt { .. }
                | ResolvedType::BitString { .. }
                | ResolvedType::Float { .. }
                | ResolvedType::Time
                | ResolvedType::Date
                | ResolvedType::Tod
                | ResolvedType::Dt
        )
    }

    /// Resolves a runtime target to a pointer in the current block,
    /// stepping into the instance struct for function block members.
    fn runtime_target_ptr(&self, target: &RuntimeTarget<'ctx>) -> CgResult<PointerValue<'ctx>> {
        match target.member {
            None => Ok(target.base),
            Some((struct_type, index)) => self
                .builder
                .build_struct_gep(struct_type, target.base, index, "member")
                .map_err(|e| CodegenError {
                    message: format!("cannot address runtime member: {}", e),
                }),
        }
    }

    /// Emits a getter that reads a runtime value and returns it as a double.
    fn emit_runtime_getter(&mut self, target: &RuntimeTarget<'ctx>, fn_name: &str) -> CgResult<()> {
        let rt = &target.resolved_type;
        let f64_type = self.context.f64_type();
        let fn_type = f64_type.fn_type(&[], false);
        let function = self.module.add_function(fn_name, fn_type, None);
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        let ptr = self.runtime_target_ptr(target)?;
        let raw = self
            .builder
            .build_load(target.llvm_type, ptr, "val")
            .unwrap();

        let as_f64 = match rt {
            ResolvedType::Float { bits: 64 } => raw.into_float_value(),
            ResolvedType::Float { .. } => self
                .builder
                .build_float_ext(raw.into_float_value(), f64_type, "fext")
                .unwrap(),
            ResolvedType::Bool => self
                .builder
                .build_unsigned_int_to_float(raw.into_int_value(), f64_type, "btof")
                .unwrap(),
            _ if rt.is_integer() => {
                if rt.is_signed() {
                    self.builder
                        .build_signed_int_to_float(raw.into_int_value(), f64_type, "itof")
                        .unwrap()
                } else {
                    self.builder
                        .build_unsigned_int_to_float(raw.into_int_value(), f64_type, "utof")
                        .unwrap()
                }
            }
            ResolvedType::Time | ResolvedType::Date | ResolvedType::Tod | ResolvedType::Dt => self
                .builder
                .build_signed_int_to_float(raw.into_int_value(), f64_type, "ttof")
                .unwrap(),
            _ => f64_type.const_float(0.0),
        };

        self.builder.build_return(Some(&as_f64)).unwrap();
        Ok(())
    }

    /// Emits a setter that accepts an f64 and stores it in the target.
    fn emit_runtime_setter(&mut self, target: &RuntimeTarget<'ctx>, fn_name: &str) -> CgResult<()> {
        let rt = &target.resolved_type;
        let f64_type = self.context.f64_type();
        let fn_type = self.context.void_type().fn_type(&[f64_type.into()], false);
        let function = self.module.add_function(fn_name, fn_type, None);
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        let ptr = self.runtime_target_ptr(target)?;
        let input = function
            .get_first_param()
            .expect("runtime setter has one parameter")
            .into_float_value();

        let value = match rt {
            ResolvedType::Float { bits: 64 } => input.into(),
            ResolvedType::Float { .. } => self
                .builder
                .build_float_trunc(input, self.context.f32_type(), "ftrunc")
                .unwrap()
                .into(),
            ResolvedType::Bool => {
                let zero = f64_type.const_zero();
                self.builder
                    .build_float_compare(FloatPredicate::ONE, input, zero, "btobool")
                    .unwrap()
                    .into()
            }
            ResolvedType::UnsignedInt { bits } | ResolvedType::BitString { bits } => self
                .builder
                .build_float_to_unsigned_int(
                    input,
                    self.context.custom_width_int_type(*bits),
                    "ftou",
                )
                .unwrap()
                .into(),
            ResolvedType::SignedInt { bits } => self
                .builder
                .build_float_to_signed_int(input, self.context.custom_width_int_type(*bits), "ftoi")
                .unwrap()
                .into(),
            ResolvedType::Time | ResolvedType::Date | ResolvedType::Tod | ResolvedType::Dt => self
                .builder
                .build_float_to_signed_int(input, self.context.i64_type(), "ftot")
                .unwrap()
                .into(),
            _ => self.zero_value(rt),
        };

        self.builder.build_store(ptr, value).unwrap();
        self.builder.build_return(None).unwrap();
        Ok(())
    }
}

// ─── Unit Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn compile_to_ir(src: &str) -> String {
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer);
        let ast = parser
            .parse()
            .unwrap_or_else(|e| panic!("Parse error: {}", e));

        let context = Context::create();
        let mut codegen = CodeGenerator::new(&context, "test");
        codegen
            .compile(&ast)
            .unwrap_or_else(|e| panic!("Codegen error: {}", e));
        codegen.ir_string()
    }

    fn compile_runtime(src: &str) -> (String, Vec<RuntimeVar>) {
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer);
        let ast = parser
            .parse()
            .unwrap_or_else(|e| panic!("Parse error: {}", e));

        let context = Context::create();
        let mut codegen = CodeGenerator::new(&context, "test");
        let vars = codegen
            .compile_for_runtime(&ast)
            .unwrap_or_else(|e| panic!("Codegen error: {}", e));
        (codegen.ir_string(), vars)
    }

    #[test]
    fn test_empty_program() {
        let ir = compile_to_ir("PROGRAM P END_PROGRAM");
        assert!(ir.contains("define void @P()"));
        assert!(ir.contains("ret void"));
    }

    #[test]
    fn test_variable_declaration() {
        let ir = compile_to_ir("PROGRAM P VAR x : INT := 42; END_VAR END_PROGRAM");
        assert!(ir.contains("alloca i16"));
        assert!(ir.contains("store i16"));
    }

    #[test]
    fn test_assignment() {
        let ir = compile_to_ir("PROGRAM P VAR x : INT := 0; END_VAR x := 10; END_PROGRAM");
        assert!(ir.contains("store i16"));
    }

    #[test]
    fn test_arithmetic() {
        let ir = compile_to_ir("PROGRAM P VAR x : INT := 0; END_VAR x := x + 1; END_PROGRAM");
        assert!(ir.contains("add"));
    }

    #[test]
    fn test_if_statement() {
        let ir = compile_to_ir(
            "PROGRAM P VAR x : BOOL := TRUE; END_VAR \
             IF x THEN x := FALSE; END_IF; END_PROGRAM",
        );
        assert!(ir.contains("br i1"));
        assert!(ir.contains("if.then"));
        assert!(ir.contains("if.merge"));
    }

    #[test]
    fn test_for_loop() {
        let ir = compile_to_ir(
            "PROGRAM P VAR i : INT; END_VAR \
             FOR i := 0 TO 10 DO i := i; END_FOR; END_PROGRAM",
        );
        assert!(ir.contains("for.cond"));
        assert!(ir.contains("for.body"));
        assert!(ir.contains("for.inc"));
    }

    #[test]
    fn test_while_loop() {
        let ir = compile_to_ir(
            "PROGRAM P VAR x : BOOL := TRUE; END_VAR \
             WHILE x DO x := FALSE; END_WHILE; END_PROGRAM",
        );
        assert!(ir.contains("while.cond"));
        assert!(ir.contains("while.body"));
    }

    #[test]
    fn test_float_ops() {
        let ir = compile_to_ir("PROGRAM P VAR x : REAL := 0.0; END_VAR x := x + 1.0; END_PROGRAM");
        assert!(ir.contains("fadd"));
    }

    #[test]
    fn test_bool_ops() {
        let ir = compile_to_ir(
            "PROGRAM P VAR a : BOOL; b : BOOL; c : BOOL; END_VAR \
             c := a AND b; END_PROGRAM",
        );
        assert!(ir.contains("and"));
    }

    #[test]
    fn test_comparison() {
        let ir = compile_to_ir(
            "PROGRAM P VAR x : INT := 0; flag : BOOL; END_VAR \
             flag := x > 0; END_PROGRAM",
        );
        assert!(ir.contains("icmp sgt"));
    }

    #[test]
    fn test_function_declaration() {
        let ir = compile_to_ir(
            "FUNCTION Add : INT VAR_INPUT a : INT; b : INT; END_VAR \
             Add := a + b; END_FUNCTION",
        );
        assert!(ir.contains("define i16 @Add(i16"));
        assert!(ir.contains("ret i16"));
    }

    #[test]
    fn test_array_access() {
        let ir = compile_to_ir(
            "PROGRAM P VAR a : ARRAY[0..9] OF DINT; i : INT := 0; END_VAR \
             a[i] := 42; END_PROGRAM",
        );
        assert!(ir.contains("getelementptr"));
    }

    #[test]
    fn test_runtime_metadata_exposes_scalar_program_vars() {
        let (ir, vars) = compile_runtime(
            "PROGRAM P VAR x : DINT := 1; run : BOOL := TRUE; data : ARRAY[0..3] OF DINT; END_VAR \
             x := x + 1; END_PROGRAM",
        );

        assert_eq!(vars.len(), 2);
        assert!(vars.iter().any(|v| {
            v.name == "x"
                && v.getter_fn_name == "__get_P_x"
                && v.setter_fn_name.as_deref() == Some("__set_P_x")
        }));
        assert!(vars.iter().any(|v| v.name == "run"));
        assert!(ir.contains("define double @__get_P_x()"));
        assert!(ir.contains("define void @__set_P_x(double"));
        assert!(!vars.iter().any(|v| v.name == "data"));
    }

    #[test]
    fn test_unary_neg() {
        let ir = compile_to_ir("PROGRAM P VAR x : INT := 0; END_VAR x := -x; END_PROGRAM");
        // int neg is `sub 0, x` in LLVM
        assert!(ir.contains("sub") || ir.contains("neg"));
    }

    #[test]
    fn test_conveyor_compiles() {
        let ir = compile_to_ir(
            r#"
PROGRAM ConveyorControl
VAR
    speed : REAL := 0.0;
    running : BOOL := FALSE;
    count : INT := 0;
    limit : INT := 1000;
    i : INT;
    sensor_vals : ARRAY[0..7] OF DINT;
END_VAR

IF running AND speed > 0.0 THEN
    count := count + 1;
    IF count >= limit THEN
        running := FALSE;
        speed := 0.0;
    ELSIF speed > 100.0 THEN
        speed := 100.0;
    END_IF;
END_IF;

FOR i := 0 TO 7 BY 1 DO
    sensor_vals[i] := sensor_vals[i] + 1;
END_FOR;

END_PROGRAM
"#,
        );
        assert!(ir.contains("define void @ConveyorControl()"));
        assert!(ir.contains("for.cond"));
        assert!(ir.contains("if.then"));
        assert!(ir.contains("getelementptr"));
    }
}
