//! C code emitter for the Arandu backend.
//!
//! [`CEmitter`] takes a fully optimized [`AmirProgram`] and produces a
//! single self-contained C translation unit as a `String`. The generated
//! code relies on GCC/Clang GNU extensions (statement expressions `({ })`)
//! and is not standard C99.

use arandu_middle::amir::{
    AmirFunc, AmirOperand, AmirProgram, AmirProjection, AmirRvalue, AmirStmt, AmirTerminator,
};
use arandu_middle::layout::{LayoutEngine, StructLayoutProvider};
use arandu_middle::literal_pool::AmirLiteralEntry;
use arandu_middle::types::{ArType, TypeInterner};
use arandu_semantics::SymbolTable;
use std::fmt::Write;

pub mod expr;
pub mod format;
pub mod stmt;

pub(super) fn sanitize_c_ident(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Emits a full C translation unit from an [`AmirProgram`].
///
/// The emitter is single-use: construct it with [`CEmitter::new`] and call
/// [`CEmitter::emit`] once to obtain the generated source as a `String`.
pub struct CEmitter<'a> {
    pub(super) program: &'a AmirProgram,
    pub(super) symbols: &'a SymbolTable,
    pub(super) layout: &'a LayoutEngine,
    pub(super) provider: &'a dyn StructLayoutProvider,
    pub(super) interner: &'a TypeInterner,
    pub(super) output: String,
    pub(super) emitted_types: rustc_hash::FxHashSet<String>,
}

impl<'a> CEmitter<'a> {
    /// Creates a new `CEmitter` bound to the given program and type metadata.
    pub fn new(
        program: &'a AmirProgram,
        symbols: &'a SymbolTable,
        layout: &'a LayoutEngine,
        provider: &'a dyn StructLayoutProvider,
        interner: &'a TypeInterner,
    ) -> Self {
        Self {
            program,
            symbols,
            layout,
            provider,
            interner,
            output: String::new(),
            emitted_types: rustc_hash::FxHashSet::default(),
        }
    }

    /// Resolve an AMIR temp's dense `TypeId` (DoD — no `ArType` on the IR).
    #[inline]
    pub(super) fn temp_ty(
        &self,
        func: &AmirFunc,
        t: arandu_middle::amir::TempId,
    ) -> ArType {
        self.interner.resolve(func.temps[t.as_usize()].ty)
    }

    #[inline]
    pub(super) fn local_ty(
        &self,
        func: &AmirFunc,
        local: arandu_middle::amir::LocalId,
    ) -> ArType {
        self.interner.resolve(func.locals[local.as_usize()].ty)
    }

    /// Emits all type definitions, string literal globals, and function bodies,
    /// then returns the complete C source as a `String`.
    pub fn emit(mut self) -> String {
        self.emit_headers();
        self.emit_str_literals();

        for func in &self.program.funcs {
            let ret = self.interner.resolve(func.return_type);
            self.ensure_type_emitted(&ret);
            for local in &func.locals {
                let ty = self.interner.resolve(local.ty);
                self.ensure_type_emitted(&ty);
            }
            for temp in &func.temps {
                let ty = self.interner.resolve(temp.ty);
                self.ensure_type_emitted(&ty);
            }
            self.emit_func_decl(func);
        }
        for func in &self.program.funcs {
            self.emit_func(func);
        }
        self.output
    }

    fn emit_headers(&mut self) {
        writeln!(&mut self.output, "#include <stdint.h>").unwrap();
        writeln!(&mut self.output, "#include <stdbool.h>").unwrap();
        writeln!(&mut self.output, "#include <stdlib.h>").unwrap();
        writeln!(&mut self.output, "#include <string.h>").unwrap();
        writeln!(&mut self.output, "#include <stdarg.h>").unwrap();
        writeln!(&mut self.output, "#ifndef AR_UNREACHABLE").unwrap();
        writeln!(&mut self.output, "#define AR_UNREACHABLE() abort()").unwrap();
        writeln!(&mut self.output, "#endif").unwrap();
        writeln!(&mut self.output).unwrap();
        // ArStr = LayoutEngine fat pointer: { ptr, len:usize } (target-dependent width).
        let len_c_ty = if self.layout.pointer_width() == 4 {
            "int32_t"
        } else {
            "int64_t"
        };
        writeln!(
            &mut self.output,
            "typedef struct {{ const uint8_t *ptr; {len_c_ty} len; }} ArStr;"
        )
        .unwrap();
        self.emitted_types.insert("ArStr".to_string());
        // Runtime helpers for fat-pointer strings (string interpolation).
        writeln!(&mut self.output, "static inline void ar_str_unpack(ArStr s, const uint8_t **ptr, {len_c_ty} *len) {{").unwrap();
        writeln!(&mut self.output, "    *ptr = s.ptr;").unwrap();
        writeln!(&mut self.output, "    *len = s.len;").unwrap();
        writeln!(&mut self.output, "}}").unwrap();
        writeln!(&mut self.output, "static inline ArStr ar_str_pack(const uint8_t *ptr, {len_c_ty} len) {{").unwrap();
        writeln!(&mut self.output, "    return (ArStr){{ .ptr = ptr, .len = len }};").unwrap();
        writeln!(&mut self.output, "}}").unwrap();
        writeln!(&mut self.output, "static ArStr ar_str_concat_n(int n, ...) {{").unwrap();
        writeln!(&mut self.output, "    if (n <= 0) return ar_str_pack((const uint8_t*)\"\", 0);").unwrap();
        writeln!(&mut self.output, "    va_list ap;").unwrap();
        writeln!(&mut self.output, "    va_start(ap, n);").unwrap();
        writeln!(&mut self.output, "    ArStr *parts = (ArStr*)malloc((size_t)n * sizeof(ArStr));").unwrap();
        writeln!(&mut self.output, "    if (!parts) {{ va_end(ap); abort(); }}").unwrap();
        writeln!(&mut self.output, "    {len_c_ty} total = 0;").unwrap();
        writeln!(&mut self.output, "    for (int i = 0; i < n; i++) {{").unwrap();
        writeln!(&mut self.output, "        parts[i] = va_arg(ap, ArStr);").unwrap();
        writeln!(&mut self.output, "        const uint8_t *p; {len_c_ty} l;").unwrap();
        writeln!(&mut self.output, "        ar_str_unpack(parts[i], &p, &l);").unwrap();
        writeln!(&mut self.output, "        if (l > 0) total += l;").unwrap();
        writeln!(&mut self.output, "    }}").unwrap();
        writeln!(&mut self.output, "    va_end(ap);").unwrap();
        writeln!(&mut self.output, "    uint8_t *buf = (uint8_t*)malloc((size_t)total + 1);").unwrap();
        writeln!(&mut self.output, "    if (!buf) {{ free(parts); abort(); }}").unwrap();
        writeln!(&mut self.output, "    {len_c_ty} off = 0;").unwrap();
        writeln!(&mut self.output, "    for (int i = 0; i < n; i++) {{").unwrap();
        writeln!(&mut self.output, "        const uint8_t *p; {len_c_ty} l;").unwrap();
        writeln!(&mut self.output, "        ar_str_unpack(parts[i], &p, &l);").unwrap();
        writeln!(&mut self.output, "        if (l > 0 && p) {{ memcpy(buf + off, p, (size_t)l); off += l; }}").unwrap();
        writeln!(&mut self.output, "    }}").unwrap();
        writeln!(&mut self.output, "    buf[total] = 0;").unwrap();
        writeln!(&mut self.output, "    free(parts);").unwrap();
        writeln!(&mut self.output, "    return ar_str_pack(buf, total);").unwrap();
        writeln!(&mut self.output, "}}").unwrap();
        writeln!(&mut self.output).unwrap();
    }

    fn emit_str_literals(&mut self) {
        for (i, entry) in self.program.literal_pool.entries.iter().enumerate() {
            if let AmirLiteralEntry::Str(s) = entry {
                // emit global array
                write!(&mut self.output, "static const uint8_t STR_{}[] = {{", i).unwrap();
                for b in s.bytes() {
                    write!(&mut self.output, "{},", b).unwrap();
                }
                writeln!(&mut self.output, "0}};").unwrap(); // null terminator for safety

                // ArStr fat-pointer constant matching LayoutEngine (ptr + len)
                writeln!(
                    &mut self.output,
                    "static const ArStr AR_STR_{} = {{ .ptr = STR_{}, .len = {} }};",
                    i,
                    i,
                    s.len()
                )
                .unwrap();
            }
        }
    }

    fn ensure_type_emitted(&mut self, ty: &ArType) {
        let name = self.format_type(ty);
        if self.emitted_types.contains(&name)
            || name == "void"
            || name == "int32_t"
            || name == "int64_t"
            || name == "bool"
            || name == "void*"
            || name == "uint8_t"
        {
            return;
        }

        let layout = self.layout.layout_of_type(ty, self.interner, self.provider);
        if layout.size > 0 {
            writeln!(
                &mut self.output,
                "typedef struct {{ _Alignas({}) uint8_t memory[{}]; }} {};",
                layout.align, layout.size, name
            )
            .unwrap();
        } else {
            writeln!(
                &mut self.output,
                "typedef struct {{ uint8_t empty; }} {};",
                name
            )
            .unwrap(); // C doesn't like zero sized structs sometimes
        }
        self.emitted_types.insert(name);
    }

    fn emit_func_decl(&mut self, func: &AmirFunc) {
        let name = sanitize_c_ident(&self.symbols.get(func.symbol).name);
        let ret = self.interner.resolve(func.return_type);
        let ret_ty = self.format_type(&ret);
        write!(&mut self.output, "{} {}(", ret_ty, name).unwrap();
        for (i, param) in func.params.iter().enumerate() {
            if i > 0 {
                write!(&mut self.output, ", ").unwrap();
            }
            let ty = self.temp_ty(func, *param);
            let ty_str = self.format_type(&ty);
            write!(&mut self.output, "{} p{}", ty_str, param.as_usize()).unwrap();
        }
        if func.params.is_empty() {
            write!(&mut self.output, "void").unwrap();
        }
        writeln!(&mut self.output, ");").unwrap();
    }

    fn emit_func(&mut self, func: &AmirFunc) {
        let name = sanitize_c_ident(&self.symbols.get(func.symbol).name);
        let ret = self.interner.resolve(func.return_type);
        let ret_ty = self.format_type(&ret);
        write!(&mut self.output, "{} {}(", ret_ty, name).unwrap();
        for (i, param) in func.params.iter().enumerate() {
            if i > 0 {
                write!(&mut self.output, ", ").unwrap();
            }
            let ty = self.temp_ty(func, *param);
            let ty_str = self.format_type(&ty);
            write!(&mut self.output, "{} p{}", ty_str, param.as_usize()).unwrap();
        }
        if func.params.is_empty() {
            write!(&mut self.output, "void").unwrap();
        }
        writeln!(&mut self.output, ") {{").unwrap();

        // Declare locals and temps strictly at the top
        let mut used_locals = rustc_hash::FxHashSet::default();
        let mut used_temps = rustc_hash::FxHashSet::default();

        for stmt in func.stmts.payloads.iter() {
            match stmt {
                AmirStmt::Assign { lhs, rhs } => {
                    used_temps.insert(lhs.as_usize());
                    match rhs {
                        AmirRvalue::Use(op)
                        | AmirRvalue::Unary { operand: op, .. }
                        | AmirRvalue::Discriminant { value: op }
                        | AmirRvalue::EnumPayload { value: op, .. }
                        | AmirRvalue::Len(op)
                        | AmirRvalue::Alloc(op) => {
                            if let AmirOperand::Copy(t) | AmirOperand::Move(t) = op {
                                used_temps.insert(t.as_usize());
                            }
                        }
                        AmirRvalue::Binary { left, right, .. } => {
                            if let AmirOperand::Copy(t) | AmirOperand::Move(t) = left {
                                used_temps.insert(t.as_usize());
                            }
                            if let AmirOperand::Copy(t) | AmirOperand::Move(t) = right {
                                used_temps.insert(t.as_usize());
                            }
                        }
                        AmirRvalue::FieldAccess { base, .. } => {
                            if let AmirOperand::Copy(t) | AmirOperand::Move(t) = base {
                                used_temps.insert(t.as_usize());
                            }
                        }
                        AmirRvalue::StructLiteral { fields, .. } => {
                            for (_, op) in fields {
                                if let AmirOperand::Copy(t) | AmirOperand::Move(t) = op {
                                    used_temps.insert(t.as_usize());
                                }
                            }
                        }
                        AmirRvalue::IndexAccess { base, index } => {
                            if let AmirOperand::Copy(t) | AmirOperand::Move(t) = base {
                                used_temps.insert(t.as_usize());
                            }
                            if let AmirOperand::Copy(t) | AmirOperand::Move(t) = index {
                                used_temps.insert(t.as_usize());
                            }
                        }
                        AmirRvalue::Array { items } | AmirRvalue::Tuple { items } => {
                            for op in items {
                                if let AmirOperand::Copy(t) | AmirOperand::Move(t) = op {
                                    used_temps.insert(t.as_usize());
                                }
                            }
                        }
                        AmirRvalue::EnumConstruct { payload, .. } => {
                            if let Some(AmirOperand::Copy(t) | AmirOperand::Move(t)) = payload {
                                used_temps.insert(t.as_usize());
                            }
                        }
                        AmirRvalue::Load(place)
                        | AmirRvalue::Borrow(place)
                        | AmirRvalue::BorrowMut(place) => {
                            used_locals.insert(place.local.as_usize());
                            for proj in &place.projections {
                                if let AmirProjection::Index(
                                    AmirOperand::Copy(t) | AmirOperand::Move(t),
                                ) = proj
                                {
                                    used_temps.insert(t.as_usize());
                                }
                            }
                        }
                        AmirRvalue::StringInterp { parts } => {
                            for op in parts {
                                if let AmirOperand::Copy(t) | AmirOperand::Move(t) = op {
                                    used_temps.insert(t.as_usize());
                                }
                            }
                        }
                    }
                }
                AmirStmt::Store { lhs, rhs } => {
                    used_locals.insert(lhs.local.as_usize());
                    for proj in &lhs.projections {
                        if let AmirProjection::Index(AmirOperand::Copy(t) | AmirOperand::Move(t)) =
                            proj
                        {
                            used_temps.insert(t.as_usize());
                        }
                    }
                    if let AmirOperand::Copy(t) | AmirOperand::Move(t) = rhs {
                        used_temps.insert(t.as_usize());
                    }
                }
                AmirStmt::Call { lhs, callee, args } => {
                    if let Some(t) = lhs {
                        used_temps.insert(t.as_usize());
                    }
                    if let AmirOperand::Copy(t) | AmirOperand::Move(t) = callee {
                        used_temps.insert(t.as_usize());
                    }
                    for arg in args {
                        if let AmirOperand::Copy(t) | AmirOperand::Move(t) = arg {
                            used_temps.insert(t.as_usize());
                        }
                    }
                }
                AmirStmt::Free(op) => {
                    if let AmirOperand::Copy(t) | AmirOperand::Move(t) = op {
                        used_temps.insert(t.as_usize());
                    }
                }
                AmirStmt::StorageLive(local) | AmirStmt::StorageDead(local) => {
                    used_locals.insert(local.as_usize());
                }
                AmirStmt::Destroy(place) => {
                    used_locals.insert(place.local.as_usize());
                    for proj in &place.projections {
                        if let AmirProjection::Index(AmirOperand::Copy(t) | AmirOperand::Move(t)) =
                            proj
                        {
                            used_temps.insert(t.as_usize());
                        }
                    }
                }
                AmirStmt::Nop => {}
            }
        }
        for block in &func.blocks {
            for param in &block.params {
                used_temps.insert(param.id.as_usize());
                used_locals.insert(param.local.as_usize());
            }
            match &block.terminator {
                AmirTerminator::Goto { args, .. } => {
                    for arg in args {
                        if let AmirOperand::Copy(t) | AmirOperand::Move(t) = arg {
                            used_temps.insert(t.as_usize());
                        }
                    }
                }
                AmirTerminator::Branch {
                    condition,
                    true_args,
                    false_args,
                    ..
                } => {
                    if let AmirOperand::Copy(t) | AmirOperand::Move(t) = condition {
                        used_temps.insert(t.as_usize());
                    }
                    for arg in true_args {
                        if let AmirOperand::Copy(t) | AmirOperand::Move(t) = arg {
                            used_temps.insert(t.as_usize());
                        }
                    }
                    for arg in false_args {
                        if let AmirOperand::Copy(t) | AmirOperand::Move(t) = arg {
                            used_temps.insert(t.as_usize());
                        }
                    }
                }
                AmirTerminator::SwitchInt {
                    discriminant,
                    targets,
                    otherwise,
                    ..
                } => {
                    if let AmirOperand::Copy(t) | AmirOperand::Move(t) = discriminant {
                        used_temps.insert(t.as_usize());
                    }
                    for (_, _, args) in targets {
                        for arg in args {
                            if let AmirOperand::Copy(t) | AmirOperand::Move(t) = arg {
                                used_temps.insert(t.as_usize());
                            }
                        }
                    }
                    for arg in &otherwise.1 {
                        if let AmirOperand::Copy(t) | AmirOperand::Move(t) = arg {
                            used_temps.insert(t.as_usize());
                        }
                    }
                }
                _ => {}
            }
        }
        for param in &func.params {
            used_temps.insert(param.as_usize());
        }

        for (i, local) in func.locals.iter().enumerate() {
            if used_locals.contains(&i) {
                let ty = self.interner.resolve(local.ty);
                let ty_str = self.format_type(&ty);
                writeln!(&mut self.output, "    {} l{};", ty_str, i).unwrap();
            }
        }
        for (i, temp) in func.temps.iter().enumerate() {
            if used_temps.contains(&i) {
                let ty = self.interner.resolve(temp.ty);
                let ty_str = self.format_type(&ty);
                writeln!(&mut self.output, "    {} t{};", ty_str, i).unwrap();
            }
        }

        writeln!(&mut self.output).unwrap();

        // Initialize temps from params
        for (i, _) in func.temps.iter().enumerate() {
            if func.params.iter().any(|&p| p.as_usize() == i) {
                writeln!(&mut self.output, "    t{} = p{};", i, i).unwrap();
            }
        }

        // Emit blocks
        for block in &func.blocks {
            writeln!(&mut self.output, "bb{}:", block.id.as_usize()).unwrap();
            for stmt in func.block_stmts(block.id) {
                self.emit_stmt(stmt, func);
            }
            self.emit_terminator(&block.terminator, func);
        }

        writeln!(&mut self.output, "}}").unwrap();
        writeln!(&mut self.output).unwrap();
    }
}
