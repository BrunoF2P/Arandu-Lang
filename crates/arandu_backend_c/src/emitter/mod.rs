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
    /// A3.3: unique id for `__ar_co_N` stack payload locals (multi-stmt).
    pub(super) co_stack_slot: u32,
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
            co_stack_slot: 0,
        }
    }

    /// Next `__ar_co_N` id for stack-first CoroutineReady multi-stmt emission.
    #[inline]
    pub(super) fn next_co_stack_slot(&mut self) -> u32 {
        let n = self.co_stack_slot;
        self.co_stack_slot = self.co_stack_slot.saturating_add(1);
        n
    }

    /// Resolve an AMIR temp's dense `TypeId` (DoD — no `ArType` on the IR).
    #[inline]
    pub(super) fn temp_ty(&self, func: &AmirFunc, t: arandu_middle::amir::TempId) -> ArType {
        self.interner.resolve(func.temps[t.as_usize()].ty)
    }

    #[inline]
    pub(super) fn local_ty(&self, func: &AmirFunc, local: arandu_middle::amir::LocalId) -> ArType {
        self.interner.resolve(func.locals[local.as_usize()].ty)
    }

    /// Emits all type definitions, string literal globals, and function bodies,
    /// then returns the complete C source as a `String`.
    pub fn emit(mut self) -> String {
        let needs_str = self.program_uses_str();
        let needs_println = self.program_uses_println();
        // println requires ArStr runtime even if no string literals.
        let needs_str = needs_str || needs_println;
        self.emit_headers(needs_str);
        if needs_str {
            self.emit_str_literals();
        }
        if needs_println {
            self.emit_prelude_println();
        }

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
        for (symbol, (params, ret)) in &self.program.extern_funcs {
            self.ensure_type_emitted(ret);
            for param in params {
                self.ensure_type_emitted(param);
            }
            let name = sanitize_c_ident(&self.symbols.get(*symbol).name);
            let ret_str = self.format_type(ret);
            let _ = write!(&mut self.output, "{} {}(", ret_str, name);
            for (i, param) in params.iter().enumerate() {
                if i > 0 {
                    let _ = write!(&mut self.output, ", ");
                }
                let ty_str = self.format_type(param);
                let _ = write!(&mut self.output, "{}", ty_str);
            }
            if params.is_empty() {
                let _ = write!(&mut self.output, "void");
            }
            let _ = writeln!(&mut self.output, ");");
        }
        for func in &self.program.funcs {
            self.emit_func(func);
        }
        self.output
    }

    /// True if any call targets prelude `io.println` (symbol name or C sanitization).
    fn program_uses_println(&self) -> bool {
        for func in &self.program.funcs {
            for stmt in func.stmts.payloads.iter() {
                if let AmirStmt::Call { callee, .. } = stmt
                    && let AmirOperand::FunctionRef(id) = callee
                {
                    let name = self.symbols.get(*id).name.as_str();
                    if name == "io.println" || name.ends_with(".println") || name == "println" {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Emit `io_println` matching sanitize_c_ident("io.println").
    fn emit_prelude_println(&mut self) {
        let _ = writeln!(&mut self.output, "static void io_println(ArStr s) {{");
        let _ = writeln!(
            &mut self.output,
            "    if (s.len > 0 && s.ptr) {{ fwrite(s.ptr, 1, (size_t)s.len, stdout); }}"
        );
        let _ = writeln!(&mut self.output, "    fputc('\\n', stdout);");
        let _ = writeln!(&mut self.output, "    fflush(stdout);");
        let _ = writeln!(&mut self.output, "}}");
        let _ = writeln!(&mut self.output);
    }

    /// Whether any local/temp/return or pool entry needs the ArStr runtime.
    fn program_uses_str(&self) -> bool {
        use arandu_middle::types::{ArType, Primitive};
        if self
            .program
            .literal_pool
            .entries
            .iter()
            .any(|e| matches!(e, AmirLiteralEntry::Str(_)))
        {
            return true;
        }
        for func in &self.program.funcs {
            let ret = self.interner.resolve(func.return_type);
            if matches!(ret, ArType::Primitive(Primitive::Str)) {
                return true;
            }
            for local in &func.locals {
                if matches!(
                    self.interner.resolve(local.ty),
                    ArType::Primitive(Primitive::Str)
                ) {
                    return true;
                }
            }
            for temp in &func.temps {
                if matches!(
                    self.interner.resolve(temp.ty),
                    ArType::Primitive(Primitive::Str)
                ) {
                    return true;
                }
            }
        }
        false
    }

    fn emit_gen_arena_runtime(&mut self) {
        let _ = writeln!(
            &mut self.output,
            r#"/* GenRef = (index<<32)|generation; gen mismatch aborts (F2.3.runtime). */
static struct {{ int64_t value; int used; uint32_t generation; }} ar_gen_slots[256];
static uint32_t ar_gen_free[256];
static int ar_gen_nslots = 0;
static int ar_gen_nfree = 0;
static int64_t ar_gen_insert_i64(int64_t v) {{
    uint32_t idx; uint32_t g;
    if (ar_gen_nfree > 0) {{
        idx = ar_gen_free[--ar_gen_nfree];
        g = ar_gen_slots[idx].generation + 1;
        ar_gen_slots[idx].generation = g;
        ar_gen_slots[idx].value = v;
        ar_gen_slots[idx].used = 1;
    }} else {{
        if (ar_gen_nslots >= 256) abort();
        idx = (uint32_t)ar_gen_nslots++;
        g = 0;
        ar_gen_slots[idx].generation = 0;
        ar_gen_slots[idx].value = v;
        ar_gen_slots[idx].used = 1;
    }}
    return ((int64_t)idx << 32) | (int64_t)g;
}}
static int64_t ar_gen_get_i64(int64_t r) {{
    uint32_t idx = (uint32_t)((uint64_t)r >> 32);
    uint32_t g = (uint32_t)r;
    if (idx >= (uint32_t)ar_gen_nslots || !ar_gen_slots[idx].used || ar_gen_slots[idx].generation != g)
        abort();
    return ar_gen_slots[idx].value;
}}
static int64_t ar_gen_remove_i64(int64_t r) {{
    uint32_t idx = (uint32_t)((uint64_t)r >> 32);
    uint32_t g = (uint32_t)r;
    if (idx >= (uint32_t)ar_gen_nslots || !ar_gen_slots[idx].used || ar_gen_slots[idx].generation != g)
        return 0;
    int64_t v = ar_gen_slots[idx].value;
    ar_gen_slots[idx].used = 0;
    if (ar_gen_nfree < 256) ar_gen_free[ar_gen_nfree++] = idx;
    return v;
}}"#
        );
    }

    fn emit_co_poll_runtime(&mut self) {
        // Typed await in expr.rs inlines disc/payload loads for the real C type.
        // Keep i64 helpers only for host/test parity paths that still use them.
        let _ = writeln!(
            &mut self.output,
            r#"/* A3.6: disc 0=Ready payload@8; disc 1=PendingOnce then Ready.
 * Prefer typed inline await (no i64 cast). i64 helpers remain for MVP host tests. */
static int ar_co_poll_i64(uint8_t *state, int64_t *out) {{
    uint32_t disc = *(uint32_t*)state;
    if (disc == 0) {{ *out = *(int64_t*)(state + 8); return 0; }}
    if (disc == 1) {{ *(uint32_t*)state = 0; return 1; }}
    *out = *(int64_t*)(state + 8); return 0;
}}
static int64_t ar_co_block_on_i64(uint8_t *state) {{
    int64_t out = 0;
    for (;;) {{
        if (ar_co_poll_i64(state, &out) == 0) return out;
    }}
}}"#
        );
    }

    fn emit_headers(&mut self, needs_str: bool) {
        let _ = writeln!(&mut self.output, "#include <stdint.h>");
        let _ = writeln!(&mut self.output, "#include <stdbool.h>");
        let _ = writeln!(&mut self.output, "#include <stdlib.h>");
        if needs_str {
            let _ = writeln!(&mut self.output, "#include <string.h>");
            let _ = writeln!(&mut self.output, "#include <stdarg.h>");
            let _ = writeln!(&mut self.output, "#include <stdio.h>");
            let _ = writeln!(&mut self.output, "#include <math.h>");
        }
        let _ = writeln!(&mut self.output, "#ifndef AR_UNREACHABLE");
        let _ = writeln!(&mut self.output, "#define AR_UNREACHABLE() abort()");
        let _ = writeln!(&mut self.output, "#endif");
        // F2.3.runtime: process-lifetime gen arena (i64 payload MVP; mirrors JIT host).
        self.emit_gen_arena_runtime();
        // A3.6: poll / block_on for coroutine state blobs (disc@0, payload@8).
        self.emit_co_poll_runtime();
        let _ = writeln!(&mut self.output);
        if !needs_str {
            return;
        }
        // ArStr = LayoutEngine fat pointer: { ptr, len:usize } (target-dependent width).
        let len_c_ty = if self.layout.pointer_width() == 4 {
            "int32_t"
        } else {
            "int64_t"
        };
        let _ = writeln!(
            &mut self.output,
            "typedef struct {{ const uint8_t *ptr; {len_c_ty} len; }} ArStr;"
        );
        self.emitted_types.insert("ArStr".to_string());
        // Runtime helpers for fat-pointer strings (string interpolation).
        let _ = writeln!(
            &mut self.output,
            "static inline void ar_str_unpack(ArStr s, const uint8_t **ptr, {len_c_ty} *len) {{"
        );
        let _ = writeln!(&mut self.output, "    *ptr = s.ptr;");
        let _ = writeln!(&mut self.output, "    *len = s.len;");
        let _ = writeln!(&mut self.output, "}}");
        let _ = writeln!(
            &mut self.output,
            "static inline ArStr ar_str_pack(const uint8_t *ptr, {len_c_ty} len) {{"
        );
        let _ = writeln!(
            &mut self.output,
            "    return (ArStr){{ .ptr = ptr, .len = len }};"
        );
        let _ = writeln!(&mut self.output, "}}");
        let _ = writeln!(
            &mut self.output,
            "static ArStr ar_str_concat_n(int n, ...) {{"
        );
        let _ = writeln!(
            &mut self.output,
            "    if (n <= 0) return ar_str_pack((const uint8_t*)\"\", 0);"
        );
        let _ = writeln!(&mut self.output, "    va_list ap;");
        let _ = writeln!(&mut self.output, "    va_start(ap, n);");
        let _ = writeln!(
            &mut self.output,
            "    ArStr *parts = (ArStr*)malloc((size_t)n * sizeof(ArStr));"
        );
        let _ = writeln!(
            &mut self.output,
            "    if (!parts) {{ va_end(ap); abort(); }}"
        );
        let _ = writeln!(&mut self.output, "    {len_c_ty} total = 0;");
        let _ = writeln!(&mut self.output, "    for (int i = 0; i < n; i++) {{");
        let _ = writeln!(&mut self.output, "        parts[i] = va_arg(ap, ArStr);");
        let _ = writeln!(&mut self.output, "        const uint8_t *p; {len_c_ty} l;");
        let _ = writeln!(&mut self.output, "        ar_str_unpack(parts[i], &p, &l);");
        let _ = writeln!(&mut self.output, "        if (l > 0) total += l;");
        let _ = writeln!(&mut self.output, "    }}");
        let _ = writeln!(&mut self.output, "    va_end(ap);");
        let _ = writeln!(
            &mut self.output,
            "    uint8_t *buf = (uint8_t*)malloc((size_t)total + 1);"
        );
        let _ = writeln!(
            &mut self.output,
            "    if (!buf) {{ free(parts); abort(); }}"
        );
        let _ = writeln!(&mut self.output, "    {len_c_ty} off = 0;");
        let _ = writeln!(&mut self.output, "    for (int i = 0; i < n; i++) {{");
        let _ = writeln!(&mut self.output, "        const uint8_t *p; {len_c_ty} l;");
        let _ = writeln!(&mut self.output, "        ar_str_unpack(parts[i], &p, &l);");
        let _ = writeln!(
            &mut self.output,
            "        if (l > 0 && p) {{ memcpy(buf + off, p, (size_t)l); off += l; }}"
        );
        let _ = writeln!(&mut self.output, "    }}");
        let _ = writeln!(&mut self.output, "    buf[total] = 0;");
        let _ = writeln!(&mut self.output, "    free(parts);");
        let _ = writeln!(&mut self.output, "    return ar_str_pack(buf, total);");
        let _ = writeln!(&mut self.output, "}}");
        // ToStr v0.1 helpers (malloc + snprintf; process-lifetime leak OK for debug).
        let _ = writeln!(&mut self.output, "static ArStr ar_i64_to_str(int64_t v) {{");
        let _ = writeln!(&mut self.output, "    char tmp[32];");
        let _ = writeln!(
            &mut self.output,
            "    int n = snprintf(tmp, sizeof(tmp), \"%lld\", (long long)v);"
        );
        let _ = writeln!(&mut self.output, "    if (n < 0) abort();");
        let _ = writeln!(
            &mut self.output,
            "    uint8_t *buf = (uint8_t*)malloc((size_t)n + 1);"
        );
        let _ = writeln!(&mut self.output, "    if (!buf) abort();");
        let _ = writeln!(&mut self.output, "    memcpy(buf, tmp, (size_t)n);");
        let _ = writeln!(&mut self.output, "    buf[n] = 0;");
        let _ = writeln!(
            &mut self.output,
            "    return ar_str_pack(buf, ({len_c_ty})n);"
        );
        let _ = writeln!(&mut self.output, "}}");
        let _ = writeln!(
            &mut self.output,
            "static ArStr ar_u64_to_str(uint64_t v) {{"
        );
        let _ = writeln!(&mut self.output, "    char tmp[32];");
        let _ = writeln!(
            &mut self.output,
            "    int n = snprintf(tmp, sizeof(tmp), \"%llu\", (unsigned long long)v);"
        );
        let _ = writeln!(&mut self.output, "    if (n < 0) abort();");
        let _ = writeln!(
            &mut self.output,
            "    uint8_t *buf = (uint8_t*)malloc((size_t)n + 1);"
        );
        let _ = writeln!(&mut self.output, "    if (!buf) abort();");
        let _ = writeln!(&mut self.output, "    memcpy(buf, tmp, (size_t)n);");
        let _ = writeln!(&mut self.output, "    buf[n] = 0;");
        let _ = writeln!(
            &mut self.output,
            "    return ar_str_pack(buf, ({len_c_ty})n);"
        );
        let _ = writeln!(&mut self.output, "}}");
        // Keep in sync with arandu_backend_cranelift::to_str_runtime::format_f64_v01
        // (specials + integer-looking values + %.15g for the rest).
        let _ = writeln!(&mut self.output, "static ArStr ar_f64_to_str(double v) {{");
        let _ = writeln!(&mut self.output, "    char tmp[64];");
        let _ = writeln!(&mut self.output, "    int n;");
        let _ = writeln!(
            &mut self.output,
            "    if (isnan(v)) {{ n = snprintf(tmp, sizeof(tmp), \"nan\"); }}"
        );
        let _ = writeln!(
            &mut self.output,
            "    else if (isinf(v)) {{ n = snprintf(tmp, sizeof(tmp), \"%s\", (v < 0) ? \"-inf\" : \"inf\"); }}"
        );
        let _ = writeln!(
            &mut self.output,
            "    else if (v == (double)(long long)v && v < 1e15 && v > -1e15) {{ n = snprintf(tmp, sizeof(tmp), \"%lld\", (long long)v); }}"
        );
        let _ = writeln!(
            &mut self.output,
            "    else {{ n = snprintf(tmp, sizeof(tmp), \"%.15g\", v); }}"
        );
        let _ = writeln!(&mut self.output, "    if (n < 0) abort();");
        let _ = writeln!(
            &mut self.output,
            "    uint8_t *buf = (uint8_t*)malloc((size_t)n + 1);"
        );
        let _ = writeln!(&mut self.output, "    if (!buf) abort();");
        let _ = writeln!(&mut self.output, "    memcpy(buf, tmp, (size_t)n);");
        let _ = writeln!(&mut self.output, "    buf[n] = 0;");
        let _ = writeln!(
            &mut self.output,
            "    return ar_str_pack(buf, ({len_c_ty})n);"
        );
        let _ = writeln!(&mut self.output, "}}");
        let _ = writeln!(&mut self.output, "static ArStr ar_bool_to_str(bool v) {{");
        let _ = writeln!(
            &mut self.output,
            "    const char *s = v ? \"true\" : \"false\";"
        );
        let _ = writeln!(&mut self.output, "    {len_c_ty} n = v ? 4 : 5;");
        let _ = writeln!(
            &mut self.output,
            "    uint8_t *buf = (uint8_t*)malloc((size_t)n + 1);"
        );
        let _ = writeln!(&mut self.output, "    if (!buf) abort();");
        let _ = writeln!(&mut self.output, "    memcpy(buf, s, (size_t)n);");
        let _ = writeln!(&mut self.output, "    buf[n] = 0;");
        let _ = writeln!(&mut self.output, "    return ar_str_pack(buf, n);");
        let _ = writeln!(&mut self.output, "}}");
        let _ = writeln!(
            &mut self.output,
            "static ArStr ar_char_to_str(uint32_t cp) {{"
        );
        let _ = writeln!(&mut self.output, "    uint8_t tmp[4];");
        let _ = writeln!(&mut self.output, "    int n = 0;");
        let _ = writeln!(
            &mut self.output,
            "    if (cp <= 0x7F) {{ tmp[0] = (uint8_t)cp; n = 1; }}"
        );
        let _ = writeln!(
            &mut self.output,
            "    else if (cp <= 0x7FF) {{ tmp[0] = (uint8_t)(0xC0 | (cp >> 6)); tmp[1] = (uint8_t)(0x80 | (cp & 0x3F)); n = 2; }}"
        );
        let _ = writeln!(
            &mut self.output,
            "    else if (cp <= 0xFFFF) {{ tmp[0] = (uint8_t)(0xE0 | (cp >> 12)); tmp[1] = (uint8_t)(0x80 | ((cp >> 6) & 0x3F)); tmp[2] = (uint8_t)(0x80 | (cp & 0x3F)); n = 3; }}"
        );
        let _ = writeln!(
            &mut self.output,
            "    else {{ tmp[0] = (uint8_t)(0xF0 | (cp >> 18)); tmp[1] = (uint8_t)(0x80 | ((cp >> 12) & 0x3F)); tmp[2] = (uint8_t)(0x80 | ((cp >> 6) & 0x3F)); tmp[3] = (uint8_t)(0x80 | (cp & 0x3F)); n = 4; }}"
        );
        let _ = writeln!(
            &mut self.output,
            "    uint8_t *buf = (uint8_t*)malloc((size_t)n + 1);"
        );
        let _ = writeln!(&mut self.output, "    if (!buf) abort();");
        let _ = writeln!(&mut self.output, "    memcpy(buf, tmp, (size_t)n);");
        let _ = writeln!(&mut self.output, "    buf[n] = 0;");
        let _ = writeln!(
            &mut self.output,
            "    return ar_str_pack(buf, ({len_c_ty})n);"
        );
        let _ = writeln!(&mut self.output, "}}");
        let _ = writeln!(&mut self.output);
    }

    /// C linkage name for a function's return type (`main` is always `int`).
    fn c_func_return_type(&self, func: &AmirFunc) -> String {
        let name = sanitize_c_ident(&self.symbols.get(func.symbol).name);
        if name == "main" {
            return "int".to_string();
        }
        let ret = self.interner.resolve(func.return_type);
        self.format_type(&ret)
    }

    fn emit_str_literals(&mut self) {
        for (i, entry) in self.program.literal_pool.entries.iter().enumerate() {
            if let AmirLiteralEntry::Str(s) = entry {
                // emit global array
                let _ = write!(&mut self.output, "static const uint8_t STR_{}[] = {{", i);
                for b in s.bytes() {
                    let _ = write!(&mut self.output, "{},", b);
                }
                let _ = writeln!(&mut self.output, "0}};"); // null terminator for safety

                // ArStr fat-pointer constant matching LayoutEngine (ptr + len)
                let _ = writeln!(
                    &mut self.output,
                    "static const ArStr AR_STR_{} = {{ .ptr = STR_{}, .len = {} }};",
                    i,
                    i,
                    s.len()
                );
            }
        }
    }

    fn ensure_type_emitted(&mut self, ty: &ArType) {
        let name = self.format_type(ty);
        // Never redefine C/stdlib primitive types as blob structs (e.g. `double`).
        if self.emitted_types.contains(&name)
            || matches!(
                name.as_str(),
                "void"
                    | "bool"
                    | "float"
                    | "double"
                    | "void*"
                    | "int8_t"
                    | "int16_t"
                    | "int32_t"
                    | "int64_t"
                    | "uint8_t"
                    | "uint16_t"
                    | "uint32_t"
                    | "uint64_t"
                    | "ArStr"
            )
        {
            return;
        }

        let layout = self.layout.layout_of_type(ty, self.interner, self.provider);
        if layout.size > 0 {
            let _ = writeln!(
                &mut self.output,
                "typedef struct {{ _Alignas({}) uint8_t memory[{}]; }} {};",
                layout.align, layout.size, name
            );
        } else {
            let _ = writeln!(
                &mut self.output,
                "typedef struct {{ uint8_t empty; }} {};",
                name
            ); // C doesn't like zero sized structs sometimes
        }
        self.emitted_types.insert(name);
    }

    fn emit_func_decl(&mut self, func: &AmirFunc) {
        let name = sanitize_c_ident(&self.symbols.get(func.symbol).name);
        let ret_ty = self.c_func_return_type(func);
        let _ = write!(&mut self.output, "{} {}(", ret_ty, name);
        for (i, param) in func.params.iter().enumerate() {
            if i > 0 {
                let _ = write!(&mut self.output, ", ");
            }
            let ty = self.temp_ty(func, *param);
            let ty_str = self.format_type(&ty);
            let _ = write!(&mut self.output, "{} p{}", ty_str, param.as_usize());
        }
        if func.params.is_empty() {
            let _ = write!(&mut self.output, "void");
        }
        let _ = writeln!(&mut self.output, ");");
    }

    fn emit_func(&mut self, func: &AmirFunc) {
        let name = sanitize_c_ident(&self.symbols.get(func.symbol).name);
        let ret_ty = self.c_func_return_type(func);
        let _ = write!(&mut self.output, "{} {}(", ret_ty, name);
        for (i, param) in func.params.iter().enumerate() {
            if i > 0 {
                let _ = write!(&mut self.output, ", ");
            }
            let ty = self.temp_ty(func, *param);
            let ty_str = self.format_type(&ty);
            let _ = write!(&mut self.output, "{} p{}", ty_str, param.as_usize());
        }
        if func.params.is_empty() {
            let _ = write!(&mut self.output, "void");
        }
        let _ = writeln!(&mut self.output, ") {{");

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
                        | AmirRvalue::Alloc(op)
                        | AmirRvalue::ToStr { value: op, .. }
                        | AmirRvalue::CoroutineReady { value: op, .. } => {
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
                        AmirRvalue::RelativeBorrow { local, .. } => {
                            used_locals.insert(local.as_usize());
                        }
                        AmirRvalue::GenInsert { value }
                        | AmirRvalue::GenGet { gen_ref: value }
                        | AmirRvalue::GenRemove { gen_ref: value } => {
                            if let AmirOperand::Copy(t) | AmirOperand::Move(t) = value {
                                used_temps.insert(t.as_usize());
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
                AmirTerminator::Suspend { future, args, .. } => {
                    if let AmirOperand::Copy(t) | AmirOperand::Move(t) = future {
                        used_temps.insert(t.as_usize());
                    }
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
                let _ = writeln!(&mut self.output, "    {} l{};", ty_str, i);
            }
        }
        for (i, temp) in func.temps.iter().enumerate() {
            if used_temps.contains(&i) {
                let ty = self.interner.resolve(temp.ty);
                let ty_str = self.format_type(&ty);
                let _ = writeln!(&mut self.output, "    {} t{};", ty_str, i);
            }
        }

        let _ = writeln!(&mut self.output);

        // Initialize temps from params
        for (i, _) in func.temps.iter().enumerate() {
            if func.params.iter().any(|&p| p.as_usize() == i) {
                let _ = writeln!(&mut self.output, "    t{} = p{};", i, i);
            }
        }

        // Labels only for blocks that are jump targets (avoids -Wunused-label).
        let mut jump_targets = rustc_hash::FxHashSet::default();
        for block in &func.blocks {
            match &block.terminator {
                AmirTerminator::Goto { target, .. } => {
                    jump_targets.insert(target.as_usize());
                }
                AmirTerminator::Suspend { resume, .. } => {
                    jump_targets.insert(resume.as_usize());
                }
                AmirTerminator::Branch {
                    if_true, if_false, ..
                } => {
                    jump_targets.insert(if_true.as_usize());
                    jump_targets.insert(if_false.as_usize());
                }
                AmirTerminator::SwitchInt {
                    targets, otherwise, ..
                } => {
                    for (_, t, _) in targets {
                        jump_targets.insert(t.as_usize());
                    }
                    jump_targets.insert(otherwise.0.as_usize());
                }
                AmirTerminator::Return | AmirTerminator::Unreachable => {}
            }
        }

        // Emit blocks
        for block in &func.blocks {
            let bid = block.id.as_usize();
            if jump_targets.contains(&bid) {
                let _ = writeln!(&mut self.output, "bb{bid}:");
            }
            for stmt in func.block_stmts(block.id) {
                self.emit_stmt(stmt, func);
            }
            self.emit_terminator(&block.terminator, func);
        }

        let _ = writeln!(&mut self.output, "}}");
        let _ = writeln!(&mut self.output);
    }
}
