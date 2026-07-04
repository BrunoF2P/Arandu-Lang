//! C code emitter for the Arandu backend.
//!
//! [`CEmitter`] takes a fully optimized [`AmirProgram`] and produces a
//! single self-contained C translation unit as a `String`. The generated
//! code relies on GCC/Clang GNU extensions (statement expressions `({ })`)
//! and is not standard C99.

use arandu_middle::amir::{
    AmirConstant, AmirFunc, AmirOperand, AmirPlace, AmirProgram, AmirProjection, AmirRvalue,
    AmirStmt, AmirTerminator,
};
use arandu_middle::layout::{LayoutEngine, StructLayoutProvider};
use arandu_middle::literal_pool::AmirLiteralEntry;
use arandu_middle::ops::{BinaryOp, UnaryOp};
use arandu_middle::types::{ArType, Primitive, TypeInterner};
use arandu_semantics::SymbolTable;
use std::fmt::Write;

/// Emits a full C translation unit from an [`AmirProgram`].
///
/// The emitter is single-use: construct it with [`CEmitter::new`] and call
/// [`CEmitter::emit`] once to obtain the generated source as a `String`.
pub struct CEmitter<'a> {
    program: &'a AmirProgram,
    symbols: &'a SymbolTable,
    layout: &'a LayoutEngine,
    provider: &'a dyn StructLayoutProvider,
    interner: &'a TypeInterner,
    output: String,
    emitted_types: rustc_hash::FxHashSet<String>,
}

fn sanitize_c_ident(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
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

    /// Emits all type definitions, string literal globals, and function bodies,
    /// then returns the complete C source as a `String`.
    pub fn emit(mut self) -> String {
        self.emit_headers();
        self.emit_str_literals();

        for func in &self.program.funcs {
            self.ensure_type_emitted(&func.return_type);
            for local in &func.locals {
                self.ensure_type_emitted(&local.ty);
            }
            for temp in &func.temps {
                self.ensure_type_emitted(&temp.ty);
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
        writeln!(&mut self.output, "#ifndef AR_UNREACHABLE").unwrap();
        writeln!(&mut self.output, "#define AR_UNREACHABLE() abort()").unwrap();
        writeln!(&mut self.output, "#endif").unwrap();
        writeln!(&mut self.output).unwrap();
        // ArStr definition matching LayoutEngine Str
        writeln!(
            &mut self.output,
            "typedef struct {{ _Alignas(8) uint8_t memory[16]; }} ArStr;"
        )
        .unwrap();
        self.emitted_types.insert("ArStr".to_string());
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

                // emit ArStr fat-pointer constant (ptr + len)
                writeln!(&mut self.output, "static const ArStr AR_STR_{} = {{", i).unwrap();
                writeln!(&mut self.output, "    .memory = {{0}}").unwrap();
                writeln!(&mut self.output, "}};").unwrap();
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
        let ret_ty = self.format_type(&func.return_type);
        write!(&mut self.output, "{} {}(", ret_ty, name).unwrap();
        for (i, param) in func.params.iter().enumerate() {
            if i > 0 {
                write!(&mut self.output, ", ").unwrap();
            }
            let ty_str = self.format_type(&func.temps[param.as_usize()].ty);
            write!(&mut self.output, "{} p{}", ty_str, param.as_usize()).unwrap();
        }
        if func.params.is_empty() {
            write!(&mut self.output, "void").unwrap();
        }
        writeln!(&mut self.output, ");").unwrap();
    }

    fn emit_func(&mut self, func: &AmirFunc) {
        let name = sanitize_c_ident(&self.symbols.get(func.symbol).name);
        let ret_ty = self.format_type(&func.return_type);
        write!(&mut self.output, "{} {}(", ret_ty, name).unwrap();
        for (i, param) in func.params.iter().enumerate() {
            if i > 0 {
                write!(&mut self.output, ", ").unwrap();
            }
            let ty_str = self.format_type(&func.temps[param.as_usize()].ty);
            write!(&mut self.output, "{} p{}", ty_str, param.as_usize()).unwrap();
        }
        if func.params.is_empty() {
            write!(&mut self.output, "void").unwrap();
        }
        writeln!(&mut self.output, ") {{").unwrap();

        // Declare locals and temps strictly at the top
        for (i, local) in func.locals.iter().enumerate() {
            let ty_str = self.format_type(&local.ty);
            writeln!(&mut self.output, "    {} l{};", ty_str, i).unwrap();
        }
        for (i, temp) in func.temps.iter().enumerate() {
            let ty_str = self.format_type(&temp.ty);
            writeln!(&mut self.output, "    {} t{};", ty_str, i).unwrap();
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

    fn emit_stmt(&mut self, stmt: &AmirStmt, func: &AmirFunc) {
        match stmt {
            AmirStmt::Assign { lhs, rhs } => {
                let lhs_ty = self.format_type(&func.temps[lhs.as_usize()].ty);
                write!(&mut self.output, "    t{} = ", lhs.as_usize()).unwrap();
                self.emit_rvalue(rhs, func, &lhs_ty);
                writeln!(&mut self.output, ";").unwrap();
            }
            AmirStmt::Call { lhs, callee, args } => {
                let callee_str = self.format_operand(callee, func);
                let args_str: Vec<_> = args.iter().map(|a| self.format_operand(a, func)).collect();
                if let Some(dest) = lhs {
                    write!(&mut self.output, "    t{} = ", dest.as_usize()).unwrap();
                } else {
                    write!(&mut self.output, "    ").unwrap();
                }
                write!(&mut self.output, "{}(", callee_str).unwrap();
                for (i, arg_str) in args_str.iter().enumerate() {
                    if i > 0 {
                        write!(&mut self.output, ", ").unwrap();
                    }
                    write!(&mut self.output, "{}", arg_str).unwrap();
                }
                writeln!(&mut self.output, ");").unwrap();
            }
            AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) => {}
            _ => {
                writeln!(&mut self.output, "    // unsupported stmt").unwrap();
            }
        }
    }

    fn emit_rvalue(&mut self, rvalue: &AmirRvalue, func: &AmirFunc, expected_c_type: &str) {
        match rvalue {
            AmirRvalue::Use(op) => {
                let op_str = self.format_operand(op, func);
                write!(&mut self.output, "{}", op_str).unwrap();
            }
            AmirRvalue::Binary { op, left, right } => {
                let left_str = self.format_operand(left, func);
                let right_str = self.format_operand(right, func);
                let op_str = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    BinaryOp::Mod => "%",
                    BinaryOp::Equal => "==",
                    BinaryOp::NotEqual => "!=",
                    BinaryOp::Lt => "<",
                    BinaryOp::LtEqual => "<=",
                    BinaryOp::Gt => ">",
                    BinaryOp::GtEqual => ">=",
                    BinaryOp::And => "&&",
                    BinaryOp::Or => "||",
                    BinaryOp::BitAnd => "&",
                    BinaryOp::BitOr => "|",
                    BinaryOp::BitXor => "^",
                    BinaryOp::ShiftLeft => "<<",
                    BinaryOp::ShiftRight => ">>",
                    _ => "?",
                };
                write!(&mut self.output, "{} {} {}", left_str, op_str, right_str).unwrap();
            }
            AmirRvalue::FieldAccess { base, field } => {
                let base_ty = match base {
                    AmirOperand::Copy(t) | AmirOperand::Move(t) => &func.temps[t.as_usize()].ty,
                    _ => {
                        return write!(&mut self.output, "/* unsupported base operand */").unwrap();
                    }
                };
                let struct_ty = match base_ty {
                    ArType::Ptr(inner) => self.interner.resolve(*inner),
                    other => other,
                };
                let layout = self
                    .layout
                    .layout_of_type(struct_ty, self.interner, self.provider);
                let offset = layout.field_offsets[*field];

                let base_temp = match base {
                    AmirOperand::Copy(t) | AmirOperand::Move(t) => t.as_usize(),
                    _ => 0,
                };
                write!(
                    &mut self.output,
                    "*({}*)((uint8_t*)&t{} + {})",
                    expected_c_type, base_temp, offset
                )
                .unwrap();
            }
            AmirRvalue::Discriminant { value } => {
                let base_temp = match value {
                    AmirOperand::Copy(t) | AmirOperand::Move(t) => t.as_usize(),
                    _ => return write!(&mut self.output, "/* unsupported base */").unwrap(),
                };
                write!(
                    &mut self.output,
                    "*(int64_t*)((uint8_t*)&t{} + 0)",
                    base_temp
                )
                .unwrap();
            }
            AmirRvalue::EnumPayload {
                value,
                variant: _,
                index: _,
            } => {
                let base_temp = match value {
                    AmirOperand::Copy(t) | AmirOperand::Move(t) => t.as_usize(),
                    _ => return write!(&mut self.output, "/* unsupported base */").unwrap(),
                };

                let base_ty = &func.temps[base_temp].ty;
                let enum_ty = match base_ty {
                    ArType::Ptr(inner) => self.interner.resolve(*inner),
                    other => other,
                };
                let enum_id = match enum_ty {
                    ArType::Named(id, _) => *id,
                    _ => arandu_middle::SymbolId(0),
                };

                let mut payload_offset = 0;
                if arandu_middle::layout::StructLayoutProvider::get_enum_variants(
                    self.provider,
                    enum_id,
                )
                .is_some()
                {
                    // Tag occupies the first 8 bytes (pointer-width). Payload begins immediately after.
                    // TODO: derive tag size from the layout engine once multi-target ABI is supported.
                    let tag_size = 8usize;
                    payload_offset = tag_size;
                }
                write!(
                    &mut self.output,
                    "*({}*)((uint8_t*)&t{} + {})",
                    expected_c_type, base_temp, payload_offset
                )
                .unwrap();
            }
            AmirRvalue::EnumConstruct {
                variant_tag,
                payload,
            } => {
                // Uses a GCC statement expression ({ ... }) to construct an enum value as a single
                // C rvalue. This allows zero-initializing the storage, writing the tag (first 8
                // bytes) and optionally writing the payload (at offset 8) before returning the
                // temporary. Both gcc and clang support this GNU extension.
                // Payload type is inferred via `typeof()` to avoid a separate C type mapping.
                write!(
                    &mut self.output,
                    "({{ {} _tmp = {{0}}; *(int64_t*)&_tmp = {}; ",
                    expected_c_type, variant_tag
                )
                .unwrap();
                if let Some(p) = payload {
                    let payload_str = self.format_operand(p, func);
                    write!(
                        &mut self.output,
                        "*(typeof({})*)((uint8_t*)&_tmp + 8) = {}; ",
                        payload_str, payload_str
                    )
                    .unwrap();
                }
                write!(&mut self.output, "_tmp; }})").unwrap();
            }
            AmirRvalue::StructLiteral {
                struct_symbol,
                fields,
            } => {
                write!(&mut self.output, "({{ {} _tmp = {{0}}; ", expected_c_type).unwrap();
                let struct_ty = arandu_middle::types::ArType::Named(*struct_symbol, Vec::new());
                let layout = self
                    .layout
                    .layout_of_type(&struct_ty, self.interner, self.provider);
                for (i, (name, op)) in fields.iter().enumerate() {
                    let field_idx = match self.provider.get_struct_field_indices(*struct_symbol) {
                        Some(indices) => indices.get(name).copied().unwrap_or(i),
                        None => i,
                    };
                    let offset = layout.field_offsets[field_idx];
                    let op_str = self.format_operand(op, func);
                    write!(
                        &mut self.output,
                        "*(typeof({})*)((uint8_t*)&_tmp + {}) = {}; ",
                        op_str, offset, op_str
                    )
                    .unwrap();
                }
                write!(&mut self.output, "_tmp; }})").unwrap();
            }
            AmirRvalue::Unary { op, operand } => {
                let op_str = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                    UnaryOp::BitNot => "~",
                    UnaryOp::Await => "",
                    _ => "",
                };
                let op_val = self.format_operand(operand, func);
                write!(&mut self.output, "{}{}", op_str, op_val).unwrap();
            }
            AmirRvalue::Load(place) => {
                let place_str = self.format_place(place, func);
                write!(&mut self.output, "{}", place_str).unwrap();
            }
            AmirRvalue::Borrow(place) => {
                let place_str = self.format_place(place, func);
                write!(&mut self.output, "&{}", place_str).unwrap();
            }
            AmirRvalue::BorrowMut(place) => {
                let place_str = self.format_place(place, func);
                write!(&mut self.output, "&{}", place_str).unwrap();
            }
            _ => {
                write!(&mut self.output, "/* unsupported rvalue */").unwrap();
            }
        }
    }

    fn emit_terminator(&mut self, term: &AmirTerminator, func: &AmirFunc) {
        match term {
            AmirTerminator::Return => {
                writeln!(&mut self.output, "    return t0;").unwrap();
            }
            AmirTerminator::Goto { target, args } => {
                self.emit_block_arguments(*target, args, func, "    ");
                writeln!(&mut self.output, "    goto bb{};", target.as_usize()).unwrap();
            }
            AmirTerminator::Branch {
                condition,
                if_true,
                true_args,
                if_false,
                false_args,
            } => {
                let cond_str = self.format_operand(condition, func);
                writeln!(&mut self.output, "    if ({}) {{", cond_str).unwrap();
                self.emit_block_arguments(*if_true, true_args, func, "        ");
                writeln!(&mut self.output, "        goto bb{};", if_true.as_usize()).unwrap();
                writeln!(&mut self.output, "    }} else {{").unwrap();
                self.emit_block_arguments(*if_false, false_args, func, "        ");
                writeln!(&mut self.output, "        goto bb{};", if_false.as_usize()).unwrap();
                writeln!(&mut self.output, "    }}").unwrap();
            }
            AmirTerminator::SwitchInt {
                discriminant,
                targets,
                otherwise,
            } => {
                let discr_str = self.format_operand(discriminant, func);
                writeln!(&mut self.output, "    switch ({}) {{", discr_str).unwrap();
                for (val, target, args) in targets.iter() {
                    writeln!(&mut self.output, "        case {}:", val).unwrap();
                    self.emit_block_arguments(*target, args, func, "            ");
                    writeln!(
                        &mut self.output,
                        "            goto bb{};",
                        target.as_usize()
                    )
                    .unwrap();
                }
                writeln!(&mut self.output, "        default:").unwrap();
                self.emit_block_arguments(otherwise.0, &otherwise.1, func, "            ");
                writeln!(
                    &mut self.output,
                    "            goto bb{};",
                    otherwise.0.as_usize()
                )
                .unwrap();
                writeln!(&mut self.output, "    }}").unwrap();
            }
            AmirTerminator::Unreachable => {
                writeln!(&mut self.output, "    AR_UNREACHABLE();").unwrap();
            }
        }
    }

    fn emit_block_arguments(
        &mut self,
        target: arandu_middle::amir::BlockId,
        args: &[AmirOperand],
        func: &AmirFunc,
        indent: &str,
    ) {
        let target_block = &func.blocks[target.as_usize()];
        for (param, arg) in target_block.params.iter().zip(args.iter()) {
            let arg_str = self.format_operand(arg, func);
            writeln!(
                &mut self.output,
                "{}t{} = {};",
                indent,
                param.id.as_usize(),
                arg_str
            )
            .unwrap();
        }
    }

    fn format_operand_str(&self, op: &AmirOperand) -> String {
        match op {
            AmirOperand::Copy(t) | AmirOperand::Move(t) => format!("t{}", t.as_usize()),
            AmirOperand::FunctionRef(id) => sanitize_c_ident(&self.symbols.get(*id).name),
            AmirOperand::Constant(c) => match c {
                AmirConstant::Pool(id) => match self.program.literal_pool.get(*id) {
                    AmirLiteralEntry::Int(v) => v.clone(),
                    AmirLiteralEntry::Float(v) => v.clone(),
                    AmirLiteralEntry::Str(_) => {
                        "((ArStr){ .memory = {0} }) /* init elsewhere */".to_string()
                    }
                    AmirLiteralEntry::Char(v) => format!("'{}'", v),
                },
                AmirConstant::Bool(b) => {
                    if *b {
                        "true".to_string()
                    } else {
                        "false".to_string()
                    }
                }
                AmirConstant::Nil => "NULL".to_string(),
            },
            _ => "/* unsupported operand */".to_string(),
        }
    }

    fn format_operand(&self, op: &AmirOperand, _func: &AmirFunc) -> String {
        // Delegates to `format_operand_str` for most operands. Pool string literals are a
        // special case: they must be emitted as an `ArStr` fat-pointer (ptr + len) rather
        // than a raw pointer, using a compound-literal array cast.
        match op {
            AmirOperand::Constant(AmirConstant::Pool(id)) => {
                match self.program.literal_pool.get(*id) {
                    AmirLiteralEntry::Str(s) => {
                        // Build an ArStr fat-pointer inline: { .ptr = STR_N, .len = N }.
                        let len = s.len();
                        format!(
                            "*(ArStr*)((uint8_t*[]){{ (uint8_t*)STR_{}, (uint8_t*){} }})",
                            id.0, len
                        )
                    }
                    _ => self.format_operand_str(op),
                }
            }
            _ => self.format_operand_str(op),
        }
    }

    fn format_type(&self, ty: &ArType) -> String {
        match ty {
            ArType::Primitive(Primitive::I32)
            | ArType::Primitive(Primitive::Int)
            | ArType::IntLiteral => "int32_t".to_string(),
            ArType::Primitive(Primitive::I64) => "int64_t".to_string(),
            ArType::Primitive(Primitive::U8) | ArType::Primitive(Primitive::Byte) => {
                "uint8_t".to_string()
            }
            ArType::Primitive(Primitive::Bool) => "bool".to_string(),
            ArType::Primitive(Primitive::Str) => "ArStr".to_string(),
            ArType::Primitive(Primitive::Float) | ArType::FloatLiteral => "double".to_string(),
            ArType::Void => "void".to_string(),
            ArType::Ptr(inner) => format!("{}*", self.format_type(self.interner.resolve(*inner))),
            ArType::Named(id, _) => sanitize_c_ident(&self.symbols.get(*id).name),
            _ => format!("ArType_{}", sanitize_c_ident(&format!("{:?}", ty))),
        }
    }

    fn format_place(&self, place: &AmirPlace, func: &AmirFunc) -> String {
        let local_idx = place.local.as_usize();
        let mut current_ty = func.locals[local_idx].ty.clone();
        let mut path = format!("l{}", local_idx);

        for proj in &place.projections {
            match proj {
                AmirProjection::Field(field_symbol_id) => {
                    let struct_id = match &current_ty {
                        ArType::Named(id, _) => *id,
                        _ => arandu_middle::SymbolId(0),
                    };
                    let layout =
                        self.layout
                            .layout_of_type(&current_ty, self.interner, self.provider);
                    let field_name = self
                        .symbols
                        .get(*field_symbol_id)
                        .name
                        .rsplit('.')
                        .next()
                        .unwrap_or("");
                    let field_idx = match self.provider.get_struct_field_indices(struct_id) {
                        Some(indices) => indices.get(field_name).copied().unwrap_or(0),
                        None => 0,
                    };
                    let offset = layout.field_offsets[field_idx];

                    let field_ty = match self.provider.get_struct_fields(struct_id) {
                        Some(fields) => fields.get(field_name).cloned().unwrap_or(ArType::Error),
                        None => ArType::Error,
                    };
                    let field_c_ty = self.format_type(&field_ty);
                    path = format!("*({}*)((uint8_t*)&{} + {})", field_c_ty, path, offset);
                    current_ty = field_ty;
                }
                AmirProjection::Index(index_op) => {
                    let elem_ty = match &current_ty {
                        ArType::Array(_, inner) | ArType::Slice(inner) | ArType::Ptr(inner) => {
                            self.interner.resolve(*inner).clone()
                        }
                        _ => ArType::Error,
                    };
                    let elem_c_ty = self.format_type(&elem_ty);
                    let index_str = self.format_operand(index_op, func);

                    if matches!(current_ty, ArType::Ptr(_)) {
                        path = format!("(({}*){})[{}]", elem_c_ty, path, index_str);
                    } else if matches!(current_ty, ArType::Slice(_)) {
                        path = format!(
                            "(( {}* )(*(void**)((uint8_t*)&{} + 0)))[{}]",
                            elem_c_ty, path, index_str
                        );
                    } else {
                        path = format!("(({}*)&{})[{}]", elem_c_ty, path, index_str);
                    }
                    current_ty = elem_ty;
                }
            }
        }
        path
    }
}
