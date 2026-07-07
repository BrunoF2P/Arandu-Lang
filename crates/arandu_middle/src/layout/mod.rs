use crate::SymbolId;
use crate::index_vec::IdIndex;
use crate::types::{ArType, Primitive, TypeId, TypeInterner};
use rustc_hash::FxHashMap;

/// A compact contiguous range into a dense backing table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DenseRange {
    pub start: u32,
    pub len: u32,
}

impl DenseRange {
    #[must_use]
    pub const fn empty() -> Self {
        Self { start: 0, len: 0 }
    }

    #[must_use]
    pub const fn new(start: usize, len: usize) -> Self {
        Self {
            start: start as u32,
            len: len as u32,
        }
    }

    #[must_use]
    pub const fn start_usize(self) -> usize {
        self.start as usize
    }

    #[must_use]
    pub const fn len_usize(self) -> usize {
        self.len as usize
    }

    #[must_use]
    pub const fn end_usize(self) -> usize {
        self.start as usize + self.len as usize
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn as_range(self) -> std::ops::Range<usize> {
        self.start_usize()..self.end_usize()
    }

    #[must_use]
    pub fn iter_ids<I: IdIndex>(self) -> DenseRangeIds<I> {
        DenseRangeIds {
            next: self.start_usize(),
            end: self.end_usize(),
            _marker: std::marker::PhantomData,
        }
    }
}

pub struct DenseRangeIds<I: IdIndex> {
    next: usize,
    end: usize,
    _marker: std::marker::PhantomData<I>,
}

impl<I: IdIndex> Iterator for DenseRangeIds<I> {
    type Item = I;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.end {
            return None;
        }
        let id = I::from_usize(self.next);
        self.next += 1;
        Some(id)
    }
}

/// Physical memory layout metadata for a resolved type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeLayout {
    pub size: u64,               // Total size in bytes (including trailing padding)
    pub align: u64,              // Alignment required in bytes (power of 2)
    pub field_offsets: Vec<u64>, // Field offsets (populated for structs, tuples, etc.)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumPayloadShape {
    pub payload_ty: Option<TypeId>,
}

/// Decoupled metadata provider to resolve struct fields and generic parameters.
pub trait StructLayoutProvider {
    fn get_struct_fields(&self, struct_id: SymbolId) -> Option<&FxHashMap<String, ArType>>;
    fn get_struct_field_indices(&self, struct_id: SymbolId) -> Option<&FxHashMap<String, usize>>;
    fn get_generic_params(&self, struct_id: SymbolId) -> Option<&[SymbolId]>;
    fn get_enum_variants(&self, enum_id: SymbolId) -> Option<Vec<EnumPayloadShape>>;
}

/// The physical memory layout engine.
///
/// Target-dependent sizes (e.g. `Float`, `Int`) are derived from
/// `pointer_width`. On 64-bit targets `Float` = F64 (8 bytes); on
/// 32-bit targets `Float` = F32 (4 bytes).
///
/// TODO: When adding support for i686, wasm32, or aarch64, replace the ad-hoc
/// `pointer_width`/`float_size` fields with a proper `DataLayout` struct that
/// stores (size, abi_align, pref_align) per type class (see the full design
/// discussed in `arandu_base::target`). Key insight: `i686_sysv` requires
/// i64/f64 to have size=8 but abi_align=4, which `pointer_width` alone cannot
/// express. The `DataLayout` approach also decouples target identity (triple)
/// from data-layout rules, allowing manual overrides like `soft-float` ABI.
#[derive(Debug, Clone)]
pub struct LayoutEngine {
    pub pointer_width: u64,
    pub float_size: u64,
}

impl LayoutEngine {
    #[must_use]
    pub fn new(pointer_width: u64) -> Self {
        Self {
            pointer_width,
            float_size: pointer_width, // 8 on 64-bit → F64, 4 on 32-bit → F32
        }
    }

    /// Compute the memory layout of any canonical `TypeId`.
    pub fn layout_of(
        &self,
        type_id: TypeId,
        interner: &TypeInterner,
        provider: &dyn StructLayoutProvider,
    ) -> TypeLayout {
        self.layout_of_type(&interner.resolve(type_id), interner, provider)
    }

    /// Compute the memory layout of a structural `ArType`.
    #[tracing::instrument(
        level = "trace",
        target = "arandu_middle::layout",
        skip(self, interner, provider)
    )]
    pub fn layout_of_type(
        &self,
        ty: &ArType,
        interner: &TypeInterner,
        provider: &dyn StructLayoutProvider,
    ) -> TypeLayout {
        match ty {
            ArType::Primitive(p) => match p {
                Primitive::I8
                | Primitive::U8
                | Primitive::Byte
                | Primitive::Bool
                | Primitive::Char => TypeLayout {
                    size: 1,
                    align: 1,
                    field_offsets: Vec::new(),
                },
                Primitive::I16 | Primitive::U16 => TypeLayout {
                    size: 2,
                    align: 2,
                    field_offsets: Vec::new(),
                },
                Primitive::I32 | Primitive::U32 | Primitive::F32 => TypeLayout {
                    size: 4,
                    align: 4,
                    field_offsets: Vec::new(),
                },
                Primitive::Float => {
                    let align = self.float_size;
                    TypeLayout {
                        size: self.float_size,
                        align,
                        field_offsets: Vec::new(),
                    }
                }
                Primitive::I64 | Primitive::U64 | Primitive::F64 => TypeLayout {
                    size: 8,
                    align: 8,
                    field_offsets: Vec::new(),
                },
                Primitive::Int | Primitive::Uint => {
                    let align = self.pointer_width;
                    TypeLayout {
                        size: self.pointer_width,
                        align,
                        field_offsets: Vec::new(),
                    }
                }
                Primitive::Str => {
                    // String (Fat Pointer): ptr: ptr[u8] (size pointer_width) + len: u64 (size 8)
                    let ptr_align = self.pointer_width;
                    let len_align = 8;
                    let max_align = ptr_align.max(len_align);

                    let ptr_offset = 0;
                    let len_offset =
                        (ptr_offset + self.pointer_width + len_align - 1) & !(len_align - 1);
                    let total_size = (len_offset + 8 + max_align - 1) & !(max_align - 1);

                    TypeLayout {
                        size: total_size,
                        align: max_align,
                        field_offsets: vec![ptr_offset, len_offset],
                    }
                }
                Primitive::Any => {
                    // Any is dynamic box pointer
                    TypeLayout {
                        size: self.pointer_width,
                        align: self.pointer_width,
                        field_offsets: Vec::new(),
                    }
                }
            },
            ArType::IntLiteral => {
                let align = self.pointer_width;
                TypeLayout {
                    size: self.pointer_width,
                    align,
                    field_offsets: Vec::new(),
                }
            }
            ArType::FloatLiteral => TypeLayout {
                size: 8,
                align: 8,
                field_offsets: Vec::new(),
            },
            ArType::Void | ArType::Err | ArType::Error => TypeLayout {
                size: 0,
                align: 1,
                field_offsets: Vec::new(),
            },
            ArType::Ptr(_) => TypeLayout {
                size: self.pointer_width,
                align: self.pointer_width,
                field_offsets: Vec::new(),
            },
            ArType::Nullable(inner) => {
                // If it is inner nullable, layout matches inner size/alignment (since ptr can be null)
                self.layout_of(*inner, interner, provider)
            }
            ArType::Slice(_) => {
                // Slice (Fat Pointer): ptr + len
                let ptr_align = self.pointer_width;
                let len_align = 8;
                let max_align = ptr_align.max(len_align);

                let ptr_offset = 0;
                let len_offset =
                    (ptr_offset + self.pointer_width + len_align - 1) & !(len_align - 1);
                let total_size = (len_offset + 8 + max_align - 1) & !(max_align - 1);

                TypeLayout {
                    size: total_size,
                    align: max_align,
                    field_offsets: vec![ptr_offset, len_offset],
                }
            }
            ArType::Array(len, inner) => {
                let inner_layout = self.layout_of(*inner, interner, provider);
                TypeLayout {
                    size: inner_layout.size * len,
                    align: inner_layout.align,
                    field_offsets: Vec::new(),
                }
            }
            ArType::Tuple(tys) => {
                let mut current_offset = 0;
                let mut max_align = 1;
                let mut field_offsets = Vec::with_capacity(tys.len());

                for &ty_id in tys {
                    let layout = self.layout_of(ty_id, interner, provider);
                    max_align = max_align.max(layout.align);
                    current_offset = (current_offset + layout.align - 1) & !(layout.align - 1);
                    field_offsets.push(current_offset);
                    current_offset += layout.size;
                }

                let total_size = (current_offset + max_align - 1) & !(max_align - 1);

                TypeLayout {
                    size: total_size,
                    align: max_align,
                    field_offsets,
                }
            }
            ArType::Named(symbol_id, generic_args) => {
                if let Some(fields_def) = provider.get_struct_fields(*symbol_id) {
                    if let Some(indices_def) = provider.get_struct_field_indices(*symbol_id) {
                        // Only `idx` and `ty` are needed after the sort; avoid
                        // cloning `name` (String) by not including it in the tuple.
                        let mut fields_with_indices: Vec<(usize, ArType)> = Vec::new();
                        for (name, ty) in fields_def {
                            if let Some(&idx) = indices_def.get(name) {
                                fields_with_indices.push((idx, ty.clone()));
                            }
                        }
                        fields_with_indices.sort_by_key(|x| x.0);

                        let generic_params = provider.get_generic_params(*symbol_id).unwrap_or(&[]);
                        let subst: FxHashMap<SymbolId, TypeId> = generic_params
                            .iter()
                            .copied()
                            .zip(generic_args.iter().copied())
                            .collect();

                        let mut current_offset = 0;
                        let mut max_align = 1;
                        let mut field_offsets = Vec::with_capacity(fields_with_indices.len());

                        for (_, ty) in fields_with_indices {
                            let substituted = substitute(&ty, &subst, interner);
                            let layout = self.layout_of_type(&substituted, interner, provider);
                            max_align = max_align.max(layout.align);
                            current_offset =
                                (current_offset + layout.align - 1) & !(layout.align - 1);
                            field_offsets.push(current_offset);
                            current_offset += layout.size;
                        }

                        let total_size = (current_offset + max_align - 1) & !(max_align - 1);

                        TypeLayout {
                            size: total_size,
                            align: max_align,
                            field_offsets,
                        }
                    } else {
                        TypeLayout {
                            size: 0,
                            align: 1,
                            field_offsets: Vec::new(),
                        }
                    }
                } else if let Some(variants) = provider.get_enum_variants(*symbol_id) {
                    let tag_size = self.pointer_width;
                    let mut max_payload_size = 0;
                    let mut max_payload_align = 1;
                    for variant in variants {
                        if let Some(payload_ty_id) = variant.payload_ty {
                            let payload_layout = self.layout_of(payload_ty_id, interner, provider);
                            if payload_layout.size > max_payload_size {
                                max_payload_size = payload_layout.size;
                            }
                            if payload_layout.align > max_payload_align {
                                max_payload_align = payload_layout.align;
                            }
                        }
                    }
                    let max_align = max_payload_align.max(tag_size);
                    let size = (tag_size + max_payload_size + max_align - 1) & !(max_align - 1);
                    TypeLayout {
                        size,
                        align: max_align,
                        field_offsets: vec![0, tag_size],
                    }
                } else {
                    TypeLayout {
                        size: 0,
                        align: 1,
                        field_offsets: Vec::new(),
                    }
                }
            }

            ArType::Func(_, _) => TypeLayout {
                size: self.pointer_width,
                align: self.pointer_width,
                field_offsets: Vec::new(),
            },
            ArType::Result(ok, err) => {
                let ok_layout = self.layout_of(*ok, interner, provider);
                let err_layout = self.layout_of(*err, interner, provider);
                let max_align = ok_layout
                    .align
                    .max(err_layout.align)
                    .max(self.pointer_width);
                let tag_offset = 0;
                let payload_offset = self.pointer_width;
                let max_payload_size = ok_layout.size.max(err_layout.size);
                let total_size =
                    (payload_offset + max_payload_size + max_align - 1) & !(max_align - 1);

                TypeLayout {
                    size: total_size,
                    align: max_align,
                    field_offsets: vec![tag_offset, payload_offset],
                }
            }
            ArType::Option(inner) => {
                let inner_layout = self.layout_of(*inner, interner, provider);
                let max_align = inner_layout.align.max(self.pointer_width);
                let tag_offset = 0;
                let payload_offset = self.pointer_width;
                let total_size =
                    (payload_offset + inner_layout.size + max_align - 1) & !(max_align - 1);

                TypeLayout {
                    size: total_size,
                    align: max_align,
                    field_offsets: vec![tag_offset, payload_offset],
                }
            }
            ArType::Coroutine(_) => TypeLayout {
                size: self.pointer_width,
                align: self.pointer_width,
                field_offsets: Vec::new(),
            },
            ArType::Range(inner) => {
                let inner_layout = self.layout_of(*inner, interner, provider);
                let align = inner_layout.align;
                let start_offset = 0;
                let end_offset = (inner_layout.size + align - 1) & !(align - 1);
                let total_size = (end_offset + inner_layout.size + align - 1) & !(align - 1);

                TypeLayout {
                    size: total_size,
                    align,
                    field_offsets: vec![start_offset, end_offset],
                }
            }
        }
    }
}

fn substitute(ty: &ArType, subst: &FxHashMap<SymbolId, TypeId>, interner: &TypeInterner) -> ArType {
    match ty {
        ArType::Named(id, args) => {
            if let Some(&concrete_id) = subst.get(id) {
                interner.resolve(concrete_id)
            } else {
                let new_args = args
                    .iter()
                    .map(|&arg_id| {
                        let arg_ty = interner.resolve(arg_id);
                        let substituted_arg = substitute(&arg_ty, subst, interner);
                        interner.lookup(&substituted_arg).unwrap_or(arg_id)
                    })
                    .collect();
                ArType::Named(*id, new_args)
            }
        }
        ArType::Func(params, ret) => {
            let new_params = params
                .iter()
                .map(|&param_id| {
                    let param_ty = interner.resolve(param_id);
                    let substituted_param = substitute(&param_ty, subst, interner);
                    interner.lookup(&substituted_param).unwrap_or(param_id)
                })
                .collect();
            let ret_ty = interner.resolve(*ret);
            let substituted_ret = substitute(&ret_ty, subst, interner);
            let new_ret = interner.lookup(&substituted_ret).unwrap_or(*ret);
            ArType::Func(new_params, new_ret)
        }
        ArType::Nullable(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            ArType::Nullable(new_inner)
        }
        ArType::Slice(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            ArType::Slice(new_inner)
        }
        ArType::Array(len, inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            ArType::Array(*len, new_inner)
        }
        ArType::Ptr(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            ArType::Ptr(new_inner)
        }
        ArType::Tuple(tys) => {
            let new_tys = tys
                .iter()
                .map(|&ty_id| {
                    let item_ty = interner.resolve(ty_id);
                    let substituted_item = substitute(&item_ty, subst, interner);
                    interner.lookup(&substituted_item).unwrap_or(ty_id)
                })
                .collect();
            ArType::Tuple(new_tys)
        }
        ArType::Result(ok, err) => {
            let ok_ty = interner.resolve(*ok);
            let substituted_ok = substitute(&ok_ty, subst, interner);
            let new_ok = interner.lookup(&substituted_ok).unwrap_or(*ok);

            let err_ty = interner.resolve(*err);
            let substituted_err = substitute(&err_ty, subst, interner);
            let new_err = interner.lookup(&substituted_err).unwrap_or(*err);

            ArType::Result(new_ok, new_err)
        }
        ArType::Option(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            ArType::Option(new_inner)
        }
        ArType::Coroutine(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            ArType::Coroutine(new_inner)
        }
        ArType::Range(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            ArType::Range(new_inner)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::newtype_index;

    newtype_index!(TestId);

    #[test]
    fn empty_range_has_no_ids() {
        let range = DenseRange::empty();
        assert!(range.is_empty());
        assert_eq!(range.as_range(), 0..0);
        assert!(range.iter_ids::<TestId>().next().is_none());
    }

    #[test]
    fn typed_iteration_returns_dense_ids() {
        assert_eq!(TestId::from_usize(9).as_usize(), 9);
        let ids: Vec<_> = DenseRange::new(2, 3).iter_ids::<TestId>().collect();
        assert_eq!(ids, vec![TestId(2), TestId(3), TestId(4)]);
    }

    struct MockProvider;
    impl StructLayoutProvider for MockProvider {
        fn get_struct_fields(&self, _struct_id: SymbolId) -> Option<&FxHashMap<String, ArType>> {
            None
        }
        fn get_struct_field_indices(
            &self,
            _struct_id: SymbolId,
        ) -> Option<&FxHashMap<String, usize>> {
            None
        }
        fn get_generic_params(&self, _struct_id: SymbolId) -> Option<&[SymbolId]> {
            None
        }
        fn get_enum_variants(&self, _enum_id: SymbolId) -> Option<Vec<EnumPayloadShape>> {
            None
        }
    }

    #[test]
    fn test_primitive_layouts_64bit() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();
        let provider = MockProvider;

        let u8_id = interner.intern(ArType::Primitive(Primitive::U8));
        let layout_u8 = engine.layout_of(u8_id, &interner, &provider);
        assert_eq!(layout_u8.size, 1);
        assert_eq!(layout_u8.align, 1);

        let i32_id = interner.intern(ArType::Primitive(Primitive::I32));
        let layout_i32 = engine.layout_of(i32_id, &interner, &provider);
        assert_eq!(layout_i32.size, 4);
        assert_eq!(layout_i32.align, 4);

        let str_id = interner.intern(ArType::Primitive(Primitive::Str));
        let layout_str = engine.layout_of(str_id, &interner, &provider);
        assert_eq!(layout_str.size, 16);
        assert_eq!(layout_str.align, 8);
        assert_eq!(layout_str.field_offsets, vec![0, 8]);
    }

    #[test]
    fn test_primitive_layouts_32bit() {
        let engine = LayoutEngine::new(4);
        let mut interner = TypeInterner::new();
        let provider = MockProvider;

        let str_id = interner.intern(ArType::Primitive(Primitive::Str));
        let layout_str = engine.layout_of(str_id, &interner, &provider);
        assert_eq!(layout_str.size, 16); // ptr (4) aligned to len (8) align -> size 16
        assert_eq!(layout_str.align, 8);
        assert_eq!(layout_str.field_offsets, vec![0, 8]);

        let int_id = interner.intern(ArType::Primitive(Primitive::Int));
        let layout_int = engine.layout_of(int_id, &interner, &provider);
        assert_eq!(layout_int.size, 4);
        assert_eq!(layout_int.align, 4);

        let uint_id = interner.intern(ArType::Primitive(Primitive::Uint));
        let layout_uint = engine.layout_of(uint_id, &interner, &provider);
        assert_eq!(layout_uint.size, 4);
        assert_eq!(layout_uint.align, 4);

        let int_lit_id = interner.intern(ArType::IntLiteral);
        let layout_int_lit = engine.layout_of(int_lit_id, &interner, &provider);
        assert_eq!(layout_int_lit.size, 4);
        assert_eq!(layout_int_lit.align, 4);
    }

    struct StructMockProvider {
        fields: FxHashMap<SymbolId, FxHashMap<String, ArType>>,
        field_indices: FxHashMap<SymbolId, FxHashMap<String, usize>>,
        generic_params: FxHashMap<SymbolId, Vec<SymbolId>>,
        enum_variants: FxHashMap<SymbolId, Vec<EnumPayloadShape>>,
    }

    impl StructLayoutProvider for StructMockProvider {
        fn get_struct_fields(&self, struct_id: SymbolId) -> Option<&FxHashMap<String, ArType>> {
            self.fields.get(&struct_id)
        }
        fn get_struct_field_indices(
            &self,
            struct_id: SymbolId,
        ) -> Option<&FxHashMap<String, usize>> {
            self.field_indices.get(&struct_id)
        }
        fn get_generic_params(&self, struct_id: SymbolId) -> Option<&[SymbolId]> {
            self.generic_params.get(&struct_id).map(|v| v.as_slice())
        }
        fn get_enum_variants(&self, enum_id: SymbolId) -> Option<Vec<EnumPayloadShape>> {
            self.enum_variants.get(&enum_id).cloned()
        }
    }

    #[test]
    fn test_all_primitive_layouts() {
        for &ptr_width in &[4u64, 8] {
            let engine = LayoutEngine::new(ptr_width);
            let mut interner = TypeInterner::new();
            let provider = MockProvider;

            let cases = [
                (Primitive::I8, 1u64, 1u64),
                (Primitive::U8, 1, 1),
                (Primitive::Byte, 1, 1),
                (Primitive::Bool, 1, 1),
                (Primitive::Char, 1, 1),
                (Primitive::I16, 2, 2),
                (Primitive::U16, 2, 2),
                (Primitive::I32, 4, 4),
                (Primitive::U32, 4, 4),
                (Primitive::F32, 4, 4),
                (Primitive::Float, ptr_width, ptr_width),
                (Primitive::Int, ptr_width, ptr_width),
                (Primitive::Uint, ptr_width, ptr_width),
                (Primitive::I64, 8, 8),
                (Primitive::U64, 8, 8),
                (Primitive::F64, 8, 8),
                (Primitive::Any, ptr_width, ptr_width),
            ];
            for (prim, size, align) in cases {
                let tid = interner.intern(ArType::Primitive(prim));
                let layout = engine.layout_of(tid, &interner, &provider);
                assert_eq!(layout.size, size, "{prim:?} size at ptr_width={ptr_width}");
                assert_eq!(
                    layout.align, align,
                    "{prim:?} align at ptr_width={ptr_width}"
                );
                assert!(layout.field_offsets.is_empty());
            }
        }
    }

    #[test]
    fn test_ptr_layout() {
        for &ptr_width in &[4u64, 8] {
            let engine = LayoutEngine::new(ptr_width);
            let mut interner = TypeInterner::new();
            let provider = MockProvider;
            let inner = interner.intern(ArType::Primitive(Primitive::I32));
            let ptr_ty = ArType::Ptr(inner);
            let tid = interner.intern(ptr_ty);
            let layout = engine.layout_of(tid, &interner, &provider);
            assert_eq!(layout.size, ptr_width);
            assert_eq!(layout.align, ptr_width);
        }
    }

    #[test]
    fn test_slice_layout() {
        for &ptr_width in &[4u64, 8] {
            let engine = LayoutEngine::new(ptr_width);
            let mut interner = TypeInterner::new();
            let provider = MockProvider;
            let inner = interner.intern(ArType::Primitive(Primitive::I32));
            let slice_ty = ArType::Slice(inner);
            let tid = interner.intern(slice_ty);
            let layout = engine.layout_of(tid, &interner, &provider);
            // ptr(ptr_width) + len(u64=8), aligned to max(ptr_width, 8)
            assert_eq!(layout.field_offsets, vec![0, ptr_width.max(8)]);
            assert_eq!(layout.align, ptr_width.max(8));
        }
    }

    #[test]
    fn test_array_layout() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();
        let provider = MockProvider;
        let elem = interner.intern(ArType::Primitive(Primitive::I32));
        let arr_ty = ArType::Array(5, elem);
        let tid = interner.intern(arr_ty);
        let layout = engine.layout_of(tid, &interner, &provider);
        assert_eq!(layout.size, 20); // 5 * 4
        assert_eq!(layout.align, 4);
    }

    #[test]
    fn test_void_error_layout() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();
        let provider = MockProvider;
        for ty in [ArType::Void, ArType::Err, ArType::Error] {
            let tid = interner.intern(ty);
            let layout = engine.layout_of(tid, &interner, &provider);
            assert_eq!(layout.size, 0);
            assert_eq!(layout.align, 1);
        }
    }

    #[test]
    fn test_func_layout() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();
        let provider = MockProvider;
        let int_id = interner.intern(ArType::Primitive(Primitive::Int));
        let func_ty = ArType::Func(vec![int_id, int_id], int_id);
        let tid = interner.intern(func_ty);
        let layout = engine.layout_of(tid, &interner, &provider);
        assert_eq!(layout.size, 8);
        assert_eq!(layout.align, 8);
    }

    #[test]
    fn test_nullable_layout_delegates_to_inner() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();
        let provider = MockProvider;
        let inner = interner.intern(ArType::Primitive(Primitive::I32));
        let nullable = ArType::Nullable(inner);
        let tid = interner.intern(nullable);
        let layout = engine.layout_of(tid, &interner, &provider);
        assert_eq!(layout.size, 4);
        assert_eq!(layout.align, 4);
    }

    #[test]
    fn test_tuple_layout() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();
        let provider = MockProvider;
        let u8_id = interner.intern(ArType::Primitive(Primitive::U8));
        let i32_id = interner.intern(ArType::Primitive(Primitive::I32));
        let u8_2 = interner.intern(ArType::Primitive(Primitive::U8));
        let tuple_ty = ArType::Tuple(vec![u8_id, i32_id, u8_2]);
        let tid = interner.intern(tuple_ty);
        let layout = engine.layout_of(tid, &interner, &provider);
        // u8 at 0, i32 at 4 (align 4), u8 at 8, total = 12 (aligned to 4)
        assert_eq!(layout.align, 4);
        assert_eq!(layout.field_offsets, vec![0, 4, 8]);
        assert_eq!(layout.size, 12);
    }

    #[test]
    fn test_result_layout() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();
        let provider = MockProvider;
        let ok = interner.intern(ArType::Primitive(Primitive::I32));
        let err = interner.intern(ArType::Primitive(Primitive::U8));
        let result_ty = ArType::Result(ok, err);
        let tid = interner.intern(result_ty);
        let layout = engine.layout_of(tid, &interner, &provider);
        // tag at 0, payload at 8 (ptr_width), max payload = max(4,1) = 4, total = 12 aligned to max(4,1,8)=8 => 16
        assert_eq!(layout.field_offsets, vec![0, 8]);
        assert_eq!(layout.align, 8);
        assert_eq!(layout.size, 16);
    }

    #[test]
    fn test_option_layout() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();
        let provider = MockProvider;
        let inner = interner.intern(ArType::Primitive(Primitive::I32));
        let opt_ty = ArType::Option(inner);
        let tid = interner.intern(opt_ty);
        let layout = engine.layout_of(tid, &interner, &provider);
        // tag at 0, payload at 8 (ptr_width), payload size 4, total = 12 aligned to max(4,8)=8 => 16
        assert_eq!(layout.field_offsets, vec![0, 8]);
        assert_eq!(layout.align, 8);
        assert_eq!(layout.size, 16);
    }

    #[test]
    fn test_range_layout() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();
        let provider = MockProvider;
        let inner = interner.intern(ArType::Primitive(Primitive::I32));
        let range_ty = ArType::Range(inner);
        let tid = interner.intern(range_ty);
        let layout = engine.layout_of(tid, &interner, &provider);
        // start at 0, end at 4 (padding to align 4), total = 8
        assert_eq!(layout.field_offsets, vec![0, 4]);
        assert_eq!(layout.align, 4);
        assert_eq!(layout.size, 8);
    }

    #[test]
    fn test_zst_allocator_field_adds_no_size() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();

        // Setup mock provider for ZST
        let mut provider = StructMockProvider {
            fields: FxHashMap::<SymbolId, FxHashMap<String, ArType>>::default(),
            field_indices: FxHashMap::<SymbolId, FxHashMap<String, usize>>::default(),
            generic_params: FxHashMap::<SymbolId, Vec<SymbolId>>::default(),
            enum_variants: FxHashMap::<SymbolId, Vec<EnumPayloadShape>>::default(),
        };

        // Create ZST struct "GlobalAllocator"
        let zst_id = SymbolId::new(0, 100);
        provider
            .fields
            .insert(zst_id, FxHashMap::<String, ArType>::default()); // No fields = ZST
        provider
            .field_indices
            .insert(zst_id, FxHashMap::<String, usize>::default());

        let zst_ty = ArType::Named(zst_id, vec![]);
        let zst_tid = interner.intern(zst_ty);
        let zst_layout = engine.layout_of(zst_tid, &interner, &provider);
        assert_eq!(zst_layout.size, 0); // ZST has 0 size
        assert_eq!(zst_layout.align, 1);

        // Create Vec struct with ZST field
        let vec_id = SymbolId::new(0, 101);
        let mut vec_fields = FxHashMap::<String, ArType>::default();
        vec_fields.insert("data".to_string(), ArType::Primitive(Primitive::I64)); // simplified pointer
        vec_fields.insert("len".to_string(), ArType::Primitive(Primitive::U64));
        vec_fields.insert("capacity".to_string(), ArType::Primitive(Primitive::U64));
        vec_fields.insert("allocator".to_string(), ArType::Named(zst_id, vec![]));

        let mut vec_indices = FxHashMap::<String, usize>::default();
        vec_indices.insert("data".to_string(), 0);
        vec_indices.insert("len".to_string(), 1);
        vec_indices.insert("capacity".to_string(), 2);
        vec_indices.insert("allocator".to_string(), 3);

        provider.fields.insert(vec_id, vec_fields);
        provider.field_indices.insert(vec_id, vec_indices);

        let vec_ty = ArType::Named(vec_id, vec![]);
        let vec_tid = interner.intern(vec_ty);
        let vec_layout = engine.layout_of(vec_tid, &interner, &provider);

        // 3 u64 fields + 1 ZST field = 3 * 8 = 24 bytes
        assert_eq!(vec_layout.size, 24);
        assert_eq!(vec_layout.align, 8);
    }

    #[test]
    fn test_int_literal_layout() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();
        let provider = MockProvider;
        let tid = interner.intern(ArType::IntLiteral);
        let layout = engine.layout_of(tid, &interner, &provider);
        assert_eq!(layout.size, 8);
        assert_eq!(layout.align, 8);
    }

    #[test]
    fn test_float_literal_layout() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();
        let provider = MockProvider;
        let tid = interner.intern(ArType::FloatLiteral);
        let layout = engine.layout_of(tid, &interner, &provider);
        assert_eq!(layout.size, 8);
        assert_eq!(layout.align, 8);
    }

    #[test]
    fn test_struct_layout_and_padding() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();

        let struct_sym = SymbolId::new(0, 1234);

        let mut fields = FxHashMap::<String, ArType>::default();
        fields.insert("a".to_string(), ArType::Primitive(Primitive::U8));
        fields.insert("b".to_string(), ArType::Primitive(Primitive::I32));
        fields.insert("c".to_string(), ArType::Primitive(Primitive::U8));

        let mut field_indices = FxHashMap::<String, usize>::default();
        field_indices.insert("a".to_string(), 0);
        field_indices.insert("b".to_string(), 1);
        field_indices.insert("c".to_string(), 2);

        let mut fields_map = FxHashMap::<SymbolId, FxHashMap<String, ArType>>::default();
        fields_map.insert(struct_sym, fields);

        let mut indices_map = FxHashMap::<SymbolId, FxHashMap<String, usize>>::default();
        indices_map.insert(struct_sym, field_indices);

        let provider = StructMockProvider {
            fields: fields_map,
            field_indices: indices_map,
            generic_params: FxHashMap::<SymbolId, Vec<SymbolId>>::default(),
            enum_variants: FxHashMap::<SymbolId, Vec<EnumPayloadShape>>::default(),
        };

        let struct_ty = ArType::Named(struct_sym, Vec::new());
        let struct_id = interner.intern(struct_ty);

        let layout = engine.layout_of(struct_id, &interner, &provider);
        // a: offset 0
        // b: offset 4 (due to align 4 of I32)
        // c: offset 8
        // total size: 12 (aligned to max alignment 4)
        assert_eq!(layout.align, 4);
        assert_eq!(layout.field_offsets, vec![0, 4, 8]);
        assert_eq!(layout.size, 12);
    }

    #[test]
    fn test_struct_missing_fields_fallback() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();
        let struct_sym = SymbolId::new(0, 9999);
        let struct_ty = ArType::Named(struct_sym, Vec::new());
        let struct_id = interner.intern(struct_ty);
        let provider = MockProvider;
        let layout = engine.layout_of(struct_id, &interner, &provider);
        assert_eq!(layout.size, 0);
        assert_eq!(layout.align, 1);
        assert!(layout.field_offsets.is_empty());
    }

    #[test]
    fn test_struct_generic_substitution() {
        let engine = LayoutEngine::new(8);
        let mut interner = TypeInterner::new();

        let struct_sym = SymbolId::new(0, 42);
        let param_sym = SymbolId::new(0, 1);

        let param_ty = ArType::Named(param_sym, vec![]);

        let mut fields = FxHashMap::<String, ArType>::default();
        fields.insert("value".to_string(), param_ty);

        let mut field_indices = FxHashMap::<String, usize>::default();
        field_indices.insert("value".to_string(), 0);

        let mut fields_map = FxHashMap::<SymbolId, FxHashMap<String, ArType>>::default();
        fields_map.insert(struct_sym, fields);

        let mut indices_map = FxHashMap::<SymbolId, FxHashMap<String, usize>>::default();
        indices_map.insert(struct_sym, field_indices);

        let mut generic_params = FxHashMap::<SymbolId, Vec<SymbolId>>::default();
        generic_params.insert(struct_sym, vec![param_sym]);

        let provider = StructMockProvider {
            fields: fields_map,
            field_indices: indices_map,
            generic_params,
            enum_variants: FxHashMap::<SymbolId, Vec<EnumPayloadShape>>::default(),
        };

        let concrete_int = interner.intern(ArType::Primitive(Primitive::I32));
        let struct_ty = ArType::Named(struct_sym, vec![concrete_int]);
        let struct_id = interner.intern(struct_ty);

        let layout = engine.layout_of(struct_id, &interner, &provider);
        // value field substituted to I32: size 4, align 4
        assert_eq!(layout.align, 4);
        assert_eq!(layout.field_offsets, vec![0]);
        assert_eq!(layout.size, 4);
    }
}
