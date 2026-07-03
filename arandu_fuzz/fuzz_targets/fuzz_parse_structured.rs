#![no_main]

use libfuzzer_sys::fuzz_target;

/// Generates structurally valid (but semantically arbitrary) Arandu source
/// from fuzz input bytes, then feeds it to the parser.
///
/// Unlike raw-byte fuzzing, this explores deep syntactic combinations instead of
/// spending most iterations on "invalid token at byte 0" rejections.
fn generate_program(bytes: &[u8]) -> String {
    let mut src = String::with_capacity(bytes.len().min(4096) + 256);
    let mut i = 0;

    src.push_str("func main() {\n");

    let stmt_count = 1 + (bytes.len() % 4);
    for _ in 0..stmt_count {
        if i >= bytes.len() {
            break;
        }
        let tag = bytes[i] % 5;
        i = (i + 1) % bytes.len();

        match tag {
            0 => {
                // var decl
                src.push_str("    let x = ");
                src.push_str(&generate_expr(bytes, &mut i));
                src.push_str(";\n");
            }
            1 => {
                // if
                src.push_str("    if ");
                src.push_str(&generate_expr(bytes, &mut i));
                src.push_str(" {\n        let _ = 1;\n    }\n");
            }
            2 => {
                // return
                src.push_str("    return ");
                i = (i + 1) % bytes.len();
                let depth = 1 + (bytes[i] as usize % 3);
                src.push_str(&generate_nested_expr(bytes, &mut i, depth));
                src.push_str(";\n");
            }
            3 => {
                // while
                src.push_str("    while ");
                src.push_str(&generate_expr(bytes, &mut i));
                src.push_str(" {\n        let _ = 1;\n    }\n");
            }
            4 => {
                // compound stmt
                src.push_str("    {\n        let _ = ");
                src.push_str(&generate_expr(bytes, &mut i));
                src.push_str(";\n    }\n");
            }
            _ => {}
        }
    }

    src.push_str("}\n");
    src
}

fn generate_expr(bytes: &[u8], i: &mut usize) -> String {
    generate_nested_expr(bytes, i, 0)
}

fn generate_nested_expr(bytes: &[u8], i: &mut usize, depth: usize) -> String {
    if *i >= bytes.len() {
        return "0".to_string();
    }

    if depth > 3 {
        // leaf — avoid runaway recursion
        return generate_leaf(bytes, i);
    }

    let tag = bytes[*i] % 7;
    *i = (*i + 1) % bytes.len();

    match tag {
        0 => generate_int(bytes, i),
        1 => generate_bool(bytes, i),
        2 => {
            // binary op
            let lhs = generate_nested_expr(bytes, i, depth + 1);
            let op = generate_op(bytes, i);
            let rhs = generate_nested_expr(bytes, i, depth + 1);
            format!("({} {} {})", lhs, op, rhs)
        }
        3 => {
            // neg
            let inner = generate_nested_expr(bytes, i, depth + 1);
            format!("(-{})", inner)
        }
        4 => {
            // not
            let inner = generate_nested_expr(bytes, i, depth + 1);
            format!("(!{})", inner)
        }
        5 => {
            // string literal
            let s = generate_string(bytes, i);
            format!("\"{}\"", s)
        }
        6 => {
            // ident (could be a call)
            let id = generate_ident(bytes, i);
            if *i < bytes.len() && bytes[*i] % 2 == 0 {
                format!("{}({})", id, generate_nested_expr(bytes, i, depth + 1))
            } else {
                id
            }
        }
        _ => "0".to_string(),
    }
}

fn generate_leaf(bytes: &[u8], i: &mut usize) -> String {
    let tag = bytes[*i] % 4;
    *i = (*i + 1) % bytes.len();
    match tag {
        0 => generate_int(bytes, i),
        1 => generate_bool(bytes, i),
        2 => generate_ident(bytes, i),
        3 => format!("\"{}\"", generate_string(bytes, i)),
        _ => "0".to_string(),
    }
}

fn generate_int(bytes: &[u8], i: &mut usize) -> String {
    let val = if *i + 4 < bytes.len() {
        let n = u32::from_ne_bytes([bytes[*i], bytes[*i + 1], bytes[*i + 2], bytes[*i + 3]]);
        *i = (*i + 4) % bytes.len();
        n % 10000
    } else {
        *i = 0;
        42
    };
    format!("{}", val as i32 - 5000)
}

fn generate_bool(bytes: &[u8], i: &mut usize) -> String {
    let b = bytes[*i] % 2 == 0;
    *i = (*i + 1) % bytes.len();
    if b { "true".to_string() } else { "false".to_string() }
}

fn generate_op(bytes: &[u8], i: &mut usize) -> &'static str {
    let ops = ["+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">=", "&&", "||"];
    let idx = bytes[*i] as usize % ops.len();
    *i = (*i + 1) % bytes.len();
    ops[idx]
}

fn generate_ident(bytes: &[u8], i: &mut usize) -> String {
    let names = ["x", "y", "z", "result", "count", "value", "tmp", "self", "data", "index"];
    let idx = bytes[*i] as usize % names.len();
    *i = (*i + 1) % bytes.len();
    names[idx].to_string()
}

fn generate_string(bytes: &[u8], i: &mut usize) -> String {
    let len = 1 + (bytes[*i] as usize % 16);
    *i = (*i + 1) % bytes.len();
    let mut s = String::with_capacity(len);
    for _ in 0..len {
        if *i >= bytes.len() {
            break;
        }
        let c = bytes[*i];
        *i = (*i + 1) % bytes.len();
        if c.is_ascii_alphanumeric() || c == b' ' {
            s.push(c as char);
        } else {
            s.push('a');
        }
    }
    s
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 || data.len() > 4096 {
        return;
    }
    let source = generate_program(data);
    if source.len() > 8192 {
        return;
    }
    let _ = arandu_parser::parse(&source);
});
