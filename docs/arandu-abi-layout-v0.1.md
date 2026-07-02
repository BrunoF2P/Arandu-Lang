# Arandu ABI Layout Specification (v0.1)

This document defines the physical memory layouts, alignment rules, and canonical ABI representation of types in the Arandu compiler.

---

## 1. Type Layout Calculation Algorithm

Memory layout in Arandu follows the standard C ABI layout rules (`#[repr(C)]`). Each type is represented by a `TypeLayout` structure:

- **Size**: Total size of the type in bytes, including internal and trailing padding.
- **Alignment**: Required boundary alignment in bytes (must be a power of two).
- **Field Offsets**: The byte offset from the start of the structure for each field (applicable to structs/tuples/results).

### Padding and Alignment Formula

The alignment of a composite type (struct or tuple) is the maximum alignment of all its fields:

$$\text{Alignment}_{\text{composite}} = \max(\text{Alignment}_{\text{field}_1}, \text{Alignment}_{\text{field}_2}, \dots)$$

When laying out fields, each field's offset must be aligned to its own alignment constraint. The formula to align an offset is:

$$\text{aligned\_offset} = (\text{offset} + \text{align} - 1) \ \& \ \sim(\text{align} - 1)$$

Finally, the total size of the composite type is aligned to the composite alignment constraint:

$$\text{aligned\_size} = (\text{total\_size} + \text{align}_{\text{composite}} - 1) \ \& \ \sim(\text{align}_{\text{composite}} - 1)$$

---

## 2. Primitive Type Layouts

The size and alignment of primitive types are defined below (under a target pointer width of $W$ bytes, where $W = 4$ or $W = 8$):

| Primitive Type | Size (Bytes) | Alignment (Bytes) | Notes |
| :--- | :--- | :--- | :--- |
| `bool`, `byte`, `char`, `i8`, `u8` | 1 | 1 | |
| `i16`, `u16` | 2 | 2 | |
| `i32`, `u32`, `f32`, `float` | 4 | 4 | Default floats are 32-bit |
| `i64`, `u64`, `f64`, `int`, `uint` | 8 | 8 | Default integers are 64-bit |
| `ptr[T]` | $W$ | $W$ | Platform-dependent pointer |
| `any` | $W$ | $W$ | Boxed dynamic pointer |
| `void`, `error` | 0 | 1 | ZSTs (Zero Sized Types) |

---

## 3. Canonical Fat Pointer Layouts (`str` and `[]T`)

Strings and Slices in Arandu are not raw pointers; they are represented using a **Fat Pointer ABI**.

### String Layout (`str`)

The layout of `str` is exactly equivalent to the following C structure:

```rust
struct StrLayout {
    ptr: ptr[u8],  // Pointer to the start of utf-8 buffer
    len: u64,      // Number of bytes in buffer (usize)
}
```

- **64-bit Target**: `size = 16`, `align = 8`, field offsets: `ptr` at offset `0`, `len` at offset `8`.
- **32-bit Target**: `size = 8`, `align = 4`, field offsets: `ptr` at offset `0`, `len` at offset `4`.

### Slice Layout (`[]T`)

Slices (`[]T`) use the same layout structure:

```rust
struct SliceLayout {
    ptr: ptr[T],   // Pointer to first element of the slice
    len: u64,      // Number of elements in the slice
}
```

- **64-bit Target**: `size = 16`, `align = 8`, field offsets: `ptr` at offset `0`, `len` at offset `8`.

---

## 4. Enums and Sum Types (`Result<T, E>` and `Option<T>`)

### `Result<T, E>` Layout

A `Result` is represented as a tagged union:

```rust
struct ResultLayout {
    tag: u64, // 0 = Ok, 1 = Err (or pointer width)
    payload: union { ok: T, err: E }
}
```

- **Alignment**: $\max(8, \text{align}(T), \text{align}(E))$
- **Offsets**: Tag at offset `0`, Payload at offset `pointer_width`.
- **Size**: $\text{align\_to}(\text{pointer\_width} + \max(\text{size}(T), \text{size}(E)), \text{Alignment})$.

### `Option<T>` Layout

Similarly:

```rust
struct OptionLayout {
    tag: u64, // 0 = None, 1 = Some (or pointer width)
    payload: T
}
```
