use crate::literal_pool::{AmirLiteralEntry, AmirLiteralPool, LiteralId};
use crate::ops::{BinaryOp, UnaryOp};
use crate::passes::type_checker::types::ArType;
use crate::{SymbolId, SymbolTable};
use smallvec::SmallVec;

#[derive(Debug)]
pub struct AmirProgram {
    pub funcs: Vec<AmirFunc>,
    pub literal_pool: AmirLiteralPool,
}

#[derive(Debug)]
pub struct AmirFunc {
    pub symbol: SymbolId,
    pub return_type: ArType,
    pub params: Vec<LocalId>,
    pub locals: Vec<AmirLocal>,
    pub blocks: Vec<AmirBasicBlock>,
}

#[derive(Debug)]
pub struct AmirLocal {
    pub id: LocalId,
    pub ty: ArType,
    pub symbol: Option<SymbolId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

impl LocalId {
    #[must_use]
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }

    #[must_use]
    pub const fn from_usize(v: usize) -> Self {
        Self(v as u32)
    }
}

impl BlockId {
    #[must_use]
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }

    #[must_use]
    pub const fn from_usize(v: usize) -> Self {
        Self(v as u32)
    }
}

#[derive(Debug)]
pub struct AmirPlace {
    pub local: LocalId,
    pub projections: SmallVec<[AmirProjection; 2]>,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum AmirProjection {
    Field(String),
    Index(AmirOperand),
}

#[derive(Debug)]
pub struct AmirBasicBlock {
    pub id: BlockId,
    pub statements: Vec<AmirStmt>,
    pub terminator: AmirTerminator,
    pub successors: Vec<BlockId>,
    pub predecessors: Vec<BlockId>,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum AmirStmt {
    Assign {
        lhs: AmirPlace,
        rhs: AmirRvalue,
    },
    Call {
        lhs: Option<LocalId>,
        callee: AmirOperand,
        args: SmallVec<[AmirOperand; 4]>,
    },
}

#[derive(Debug)]
#[non_exhaustive]
pub enum AmirRvalue {
    Use(AmirOperand),
    Binary {
        op: BinaryOp,
        left: AmirOperand,
        right: AmirOperand,
    },
    Unary {
        op: UnaryOp,
        operand: AmirOperand,
    },
    FieldAccess {
        base: AmirOperand,
        field: String,
    },
    StructLiteral {
        struct_symbol: SymbolId,
        fields: Vec<(String, AmirOperand)>,
    },
    IndexAccess {
        base: AmirOperand,
        index: AmirOperand,
    },
    Array {
        items: Vec<AmirOperand>,
    },
    Tuple {
        items: Vec<AmirOperand>,
    },
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AmirOperand {
    Copy(LocalId),
    Move(LocalId),
    Constant(AmirConstant),
    FunctionRef(SymbolId),
    GlobalRef(SymbolId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AmirConstant {
    Pool(LiteralId),
    Bool(bool),
    Nil,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum AmirTerminator {
    Return,
    Goto(BlockId),
    /// Boolean conditional branch: if `condition` is true, jump to `if_true`, else `if_false`.
    Branch {
        condition: AmirOperand,
        if_true: BlockId,
        if_false: BlockId,
    },
    /// Integer discriminant switch (e.g. enum tags, `switch` on int).
    SwitchInt {
        discriminant: AmirOperand,
        targets: Vec<(i128, BlockId)>,
        otherwise: BlockId,
    },
    Unreachable,
}

impl AmirProgram {
    pub fn pretty_print(&self, symbols: &SymbolTable) -> String {
        let mut out = String::new();
        for (i, func) in self.funcs.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            func.pretty_print_to(&mut out, symbols, &self.literal_pool);
        }
        out
    }
}

impl AmirFunc {
    fn pretty_print_to(&self, out: &mut String, symbols: &SymbolTable, pool: &AmirLiteralPool) {
        let param_strs: Vec<String> = self
            .params
            .iter()
            .map(|p| {
                let local = &self.locals[p.as_usize()];
                let name_str = local
                    .symbol
                    .map(|s| symbols.get(s).name.as_str())
                    .unwrap_or("param");
                format!("{}: {}", name_str, local.ty.display(symbols))
            })
            .collect();
        out.push_str(&format!(
            "Func {}({}) -> {}\n",
            symbols.get(self.symbol).name,
            param_strs.join(", "),
            self.return_type.display(symbols)
        ));

        out.push_str("  locals:\n");
        for local in &self.locals {
            let comment = if local.id == LocalId(0) {
                " // return".to_string()
            } else if let Some(sym) = local.symbol {
                format!(" // {}", symbols.get(sym).name)
            } else {
                String::new()
            };
            out.push_str(&format!(
                "    _{}: {}{}\n",
                local.id.0,
                local.ty.display(symbols),
                comment
            ));
        }

        out.push('\n');

        // Basic blocks
        for block in &self.blocks {
            out.push_str(&format!("  bb{}:\n", block.id.0));
            for stmt in &block.statements {
                out.push_str("    ");
                stmt.pretty_print_to(out, symbols, pool);
                out.push('\n');
            }
            out.push_str("    ");
            block.terminator.pretty_print_to(out, symbols, pool);
            out.push('\n');
        }
    }
}

impl AmirPlace {
    pub fn pretty_print_to(&self, out: &mut String, symbols: &SymbolTable, pool: &AmirLiteralPool) {
        out.push_str(&format!("_{}", self.local.0));
        for proj in &self.projections {
            match proj {
                AmirProjection::Field(name) => {
                    out.push_str(&format!(".{}", name));
                }
                AmirProjection::Index(op) => {
                    out.push_str(&format!("[{}]", op.to_pretty_string(symbols, pool)));
                }
            }
        }
    }
}

impl AmirStmt {
    fn pretty_print_to(&self, out: &mut String, symbols: &SymbolTable, pool: &AmirLiteralPool) {
        match self {
            AmirStmt::Assign { lhs, rhs } => {
                lhs.pretty_print_to(out, symbols, pool);
                out.push_str(" = ");
                rhs.pretty_print_to(out, symbols, pool);
            }
            AmirStmt::Call { lhs, callee, args } => {
                if let Some(l) = lhs {
                    out.push_str(&format!("_{} = ", l.0));
                }
                out.push_str("call ");
                callee.pretty_print_to(out, symbols, pool);
                out.push('(');
                let arg_strs: Vec<String> = args
                    .iter()
                    .map(|a| a.to_pretty_string(symbols, pool))
                    .collect();
                out.push_str(&arg_strs.join(", "));
                out.push(')');
            }
        }
    }
}

impl AmirRvalue {
    fn pretty_print_to(&self, out: &mut String, symbols: &SymbolTable, pool: &AmirLiteralPool) {
        match self {
            AmirRvalue::Use(op) => {
                op.pretty_print_to(out, symbols, pool);
            }
            AmirRvalue::Binary { op, left, right } => {
                out.push_str(&format!(
                    "{} {}, {}",
                    binary_op_name(op),
                    left.to_pretty_string(symbols, pool),
                    right.to_pretty_string(symbols, pool)
                ));
            }
            AmirRvalue::Unary { op, operand } => {
                out.push_str(&format!(
                    "{} {}",
                    unary_op_name(op),
                    operand.to_pretty_string(symbols, pool)
                ));
            }
            AmirRvalue::FieldAccess { base, field } => {
                out.push_str(&format!(
                    "{}.{}",
                    base.to_pretty_string(symbols, pool),
                    field
                ));
            }
            AmirRvalue::StructLiteral {
                struct_symbol,
                fields,
            } => {
                let struct_name = &symbols.get(*struct_symbol).name;
                out.push_str(&format!("{} {{ ", struct_name));
                let field_strs: Vec<String> = fields
                    .iter()
                    .map(|(name, op)| format!("{}: {}", name, op.to_pretty_string(symbols, pool)))
                    .collect();
                out.push_str(&field_strs.join(", "));
                out.push_str(" }");
            }
            AmirRvalue::IndexAccess { base, index } => {
                out.push_str(&format!(
                    "{}[{}]",
                    base.to_pretty_string(symbols, pool),
                    index.to_pretty_string(symbols, pool)
                ));
            }
            AmirRvalue::Array { items } => {
                let item_strs: Vec<String> = items
                    .iter()
                    .map(|a| a.to_pretty_string(symbols, pool))
                    .collect();
                out.push_str(&format!("[{}]", item_strs.join(", ")));
            }
            AmirRvalue::Tuple { items } => {
                let item_strs: Vec<String> = items
                    .iter()
                    .map(|a| a.to_pretty_string(symbols, pool))
                    .collect();
                out.push_str(&format!("({})", item_strs.join(", ")));
            }
        }
    }
}

impl AmirOperand {
    fn to_pretty_string(&self, symbols: &SymbolTable, pool: &AmirLiteralPool) -> String {
        let mut out = String::new();
        self.pretty_print_to(&mut out, symbols, pool);
        out
    }

    fn pretty_print_to(&self, out: &mut String, symbols: &SymbolTable, pool: &AmirLiteralPool) {
        match self {
            AmirOperand::Copy(l) => {
                out.push_str(&format!("_{}", l.0));
            }
            AmirOperand::Move(l) => {
                out.push_str(&format!("move _{}", l.0));
            }
            AmirOperand::Constant(c) => {
                c.pretty_print_to(out, pool);
            }
            AmirOperand::FunctionRef(sym) => {
                out.push_str(&format!("fn@{}", symbols.get(*sym).name));
            }
            AmirOperand::GlobalRef(sym) => {
                out.push_str(&format!("global@{}", symbols.get(*sym).name));
            }
        }
    }
}

impl AmirConstant {
    fn pretty_print_to(&self, out: &mut String, pool: &AmirLiteralPool) {
        match self {
            AmirConstant::Pool(id) => match pool.get(*id) {
                AmirLiteralEntry::Int(v) => out.push_str(v),
                AmirLiteralEntry::Float(v) => out.push_str(v),
                AmirLiteralEntry::Str(v) => out.push_str(&format!("\"{v}\"")),
                AmirLiteralEntry::Char(v) => out.push_str(&format!("'{v}'")),
            },
            AmirConstant::Bool(v) => out.push_str(&v.to_string()),
            AmirConstant::Nil => out.push_str("nil"),
        }
    }
}

impl AmirTerminator {
    fn pretty_print_to(&self, out: &mut String, symbols: &SymbolTable, pool: &AmirLiteralPool) {
        match self {
            AmirTerminator::Return => {
                out.push_str("return");
            }
            AmirTerminator::Goto(b) => {
                out.push_str(&format!("goto bb{}", b.0));
            }
            AmirTerminator::Branch {
                condition,
                if_true,
                if_false,
            } => {
                out.push_str(&format!(
                    "branch {} => bb{}, else bb{}",
                    condition.to_pretty_string(symbols, pool),
                    if_true.0,
                    if_false.0
                ));
            }
            AmirTerminator::SwitchInt {
                discriminant,
                targets,
                otherwise,
            } => {
                out.push_str(&format!(
                    "switchInt {} {{ ",
                    discriminant.to_pretty_string(symbols, pool)
                ));
                let target_strs: Vec<String> = targets
                    .iter()
                    .map(|(val, dest)| format!("{} => bb{}", val, dest.0))
                    .collect();
                out.push_str(&target_strs.join(", "));
                if !targets.is_empty() {
                    out.push_str(", ");
                }
                out.push_str(&format!("otherwise => bb{} }}", otherwise.0));
            }
            AmirTerminator::Unreachable => {
                out.push_str("unreachable");
            }
        }
    }
}

fn binary_op_name(op: &BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "add",
        BinaryOp::Sub => "sub",
        BinaryOp::Mul => "mul",
        BinaryOp::Div => "div",
        BinaryOp::Mod => "mod",
        BinaryOp::Equal => "eq",
        BinaryOp::NotEqual => "ne",
        BinaryOp::Lt => "lt",
        BinaryOp::LtEqual => "le",
        BinaryOp::Gt => "gt",
        BinaryOp::GtEqual => "ge",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
        BinaryOp::BitAnd => "bitand",
        BinaryOp::BitOr => "bitor",
        BinaryOp::BitXor => "bitxor",
        BinaryOp::ShiftLeft => "shl",
        BinaryOp::ShiftRight => "shr",
        BinaryOp::NullCoalesce => "null_coalesce",
        BinaryOp::RangeExclusive => "range_exclusive",
        BinaryOp::RangeInclusive => "range_inclusive",
    }
}

fn unary_op_name(op: &UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "not",
        UnaryOp::Neg => "neg",
        UnaryOp::BitNot => "bitnot",
        UnaryOp::Await => "await",
    }
}
