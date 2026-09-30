//! Bounded scalar AMIR execution. Calls use an explicit heap stack, never Rust
//! recursion. The provider owns function lookup; this module knows no database.

use std::sync::Arc;

use super::{ScalarEvalError, eval_binary, eval_unary};
use arandu_middle::amir::*;
use arandu_middle::ctfe::{ConstInt, ConstValue, ConstValueError, IntegerType};
use arandu_middle::layout::{DataLayout, DenseRange};
use arandu_middle::literal_pool::{AmirLiteralEntry, AmirLiteralPool, parse_int_literal};
use arandu_middle::ops::{BinaryOp, UnaryOp};
use arandu_middle::types::{ArType, Primitive, TypeId, TypeInterner};
use arandu_middle::{Span, SymbolId};

/// Required explicitly until representative workloads establish defaults.
/// Fuel charges validation, instructions and terminators. Live value slots
/// include frames and simultaneous edge/call argument scratch storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Budget {
    pub fuel: u64,
    pub frames: u32,
    pub values: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarType {
    Void,
    Bool,
    Integer(IntegerType),
}

impl ScalarType {
    fn resolve(
        id: TypeId,
        types: &TypeInterner,
        layout: DataLayout,
    ) -> Result<Self, EvalErrorKind> {
        match types.try_resolve(id) {
            Some(ArType::Void) => Ok(Self::Void),
            Some(ArType::Primitive(Primitive::Bool)) => Ok(Self::Bool),
            Some(ArType::Primitive(primitive)) => IntegerType::new(primitive, layout)
                .map(Self::Integer)
                .map_err(EvalErrorKind::Value),
            _ => Err(EvalErrorKind::UnsupportedType(id)),
        }
    }
    fn accepts(self, value: ConstValue) -> bool {
        match (self, value) {
            (Self::Void, ConstValue::Void) | (Self::Bool, ConstValue::Bool(_)) => true,
            (Self::Integer(ty), ConstValue::Integer(value)) => ty == value.ty(),
            _ => false,
        }
    }
}

/// A single function with its own literal pool and resolved scalar types.
/// Type/literal IDs never cross function units; the provider shares units by Arc.
#[derive(Debug)]
pub struct CtfeFunction {
    function: AmirFunc,
    literals: AmirLiteralPool,
    layout: DataLayout,
    temp_types: Vec<ScalarType>,
    local_types: Vec<ScalarType>,
    return_type: ScalarType,
}

impl CtfeFunction {
    #[must_use]
    pub fn function(&self) -> &AmirFunc {
        &self.function
    }

    #[must_use]
    pub fn literals(&self) -> &AmirLiteralPool {
        &self.literals
    }

    #[must_use]
    pub fn layout(&self) -> DataLayout {
        self.layout
    }

    pub fn new(
        function: AmirFunc,
        literals: AmirLiteralPool,
        types: &TypeInterner,
        layout: DataLayout,
    ) -> Result<Self, EvalErrorKind> {
        IntegerType::new(Primitive::USize, layout).map_err(EvalErrorKind::Value)?;
        let return_type = ScalarType::resolve(function.return_type, types, layout)?;
        let temp_types = function
            .temps
            .iter()
            .map(|temp| ScalarType::resolve(temp.ty, types, layout))
            .collect::<Result<_, _>>()?;
        let local_types = function
            .locals
            .iter()
            .map(|local| ScalarType::resolve(local.ty, types, layout))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            function,
            literals,
            layout,
            temp_types,
            local_types,
            return_type,
        })
    }

    /// Pool-independent type encodings for semantic hashing by query consumers.
    pub fn scalar_type_bytes(&self) -> impl Iterator<Item = [u8; 20]> + '_ {
        std::iter::once(&self.return_type)
            .chain(&self.temp_types)
            .chain(&self.local_types)
            .map(|ty| match *ty {
                ScalarType::Void => ConstValue::Void.canonical_bytes(),
                ScalarType::Bool => ConstValue::Bool(false).canonical_bytes(),
                ScalarType::Integer(integer) => {
                    // Zero belongs to every admitted integer type. Avoid a
                    // fallible constructor here by exposing the type encoding.
                    integer.canonical_bytes()
                }
            })
    }
}

pub trait FunctionProvider {
    fn function(&self, symbol: SymbolId) -> Result<Arc<CtfeFunction>, EvalErrorKind>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalErrorKind {
    Cancelled,
    FuelExhausted,
    FrameLimit,
    ValueLimit,
    AllocationFailed,
    MissingFunction(SymbolId),
    UnavailableFunction(SymbolId),
    UnsupportedType(TypeId),
    UnsupportedOperation,
    InvalidIr,
    Uninitialized,
    TypeMismatch,
    InvalidLiteral,
    TargetMismatch,
    Arithmetic(ScalarEvalError),
    Value(ConstValueError),
}

/// Internal failure with the current source location. Cancellation is not a
/// language error; a query boundary must unwind it instead of memoizing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalError {
    pub kind: EvalErrorKind,
    pub function: SymbolId,
    pub block: BlockId,
    pub span: Span,
}

struct Frame {
    unit: Arc<CtfeFunction>,
    temps: Vec<Option<ConstValue>>,
    locals: Vec<Option<ConstValue>>,
    block: BlockId,
    instruction: usize,
    destination: Option<TempId>,
}

struct Meter<'a, C> {
    fuel: u64,
    values: usize,
    max_values: usize,
    max_frames: usize,
    cancelled: &'a mut C,
}

impl<C: FnMut() -> bool> Meter<'_, C> {
    fn step(&mut self) -> Result<(), EvalErrorKind> {
        if (self.cancelled)() {
            return Err(EvalErrorKind::Cancelled);
        }
        self.fuel = self
            .fuel
            .checked_sub(1)
            .ok_or(EvalErrorKind::FuelExhausted)?;
        Ok(())
    }
    fn reserve(&self, count: usize) -> Result<(), EvalErrorKind> {
        if count > self.max_values.saturating_sub(self.values) {
            return Err(EvalErrorKind::ValueLimit);
        }
        Ok(())
    }
}

fn slice_range(range: DenseRange) -> Result<std::ops::Range<usize>, EvalErrorKind> {
    let start = usize::try_from(range.start).map_err(|_| EvalErrorKind::InvalidIr)?;
    let len = usize::try_from(range.len).map_err(|_| EvalErrorKind::InvalidIr)?;
    Ok(start..start.checked_add(len).ok_or(EvalErrorKind::InvalidIr)?)
}

fn slots(count: usize) -> Result<Vec<Option<ConstValue>>, EvalErrorKind> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| EvalErrorKind::AllocationFailed)?;
    values.resize(count, None);
    Ok(values)
}

impl Frame {
    fn new<C: FnMut() -> bool>(
        unit: Arc<CtfeFunction>,
        args: &[ConstValue],
        destination: Option<TempId>,
        meter: &mut Meter<'_, C>,
    ) -> Result<Self, EvalErrorKind> {
        let function = &unit.function;
        let count = function
            .temps
            .len()
            .checked_add(function.locals.len())
            .ok_or(EvalErrorKind::ValueLimit)?;
        meter.reserve(count)?;
        if function.receiver.is_some()
            || function.params.len() != args.len()
            || function.blocks.is_empty()
        {
            return Err(EvalErrorKind::InvalidIr);
        }
        // Inspect every block before executing this function, including dead
        // branches. Optimizers must not hide an inadmissible effect from CTFE.
        for (index, block) in function.blocks.iter().enumerate() {
            meter.step()?;
            if block.id.as_usize() != index
                || function
                    .block_params
                    .get(slice_range(block.params)?)
                    .is_none()
            {
                return Err(EvalErrorKind::InvalidIr);
            }
            for index in slice_range(block.statements)? {
                meter.step()?;
                let id = u32::try_from(index)
                    .map(InstrId)
                    .map_err(|_| EvalErrorKind::InvalidIr)?;
                match function.try_stmt(id).ok_or(EvalErrorKind::InvalidIr)? {
                    AmirStmt::Assign { rhs, .. } => admitted_rvalue(rhs)?,
                    AmirStmt::Store { lhs, .. } if lhs.projections.is_empty() => {}
                    AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) | AmirStmt::Nop => {}
                    AmirStmt::Call {
                        callee: AmirOperand::FunctionRef(_),
                        return_borrow: None,
                        ..
                    } => {}
                    AmirStmt::Store { .. }
                    | AmirStmt::Call { .. }
                    | AmirStmt::Free(_)
                    | AmirStmt::Destroy(_) => return Err(EvalErrorKind::UnsupportedOperation),
                }
            }
            match block.terminator {
                AmirTerminator::Return
                | AmirTerminator::Goto { .. }
                | AmirTerminator::Branch { .. }
                | AmirTerminator::SwitchInt { .. }
                | AmirTerminator::Unreachable => {}
                AmirTerminator::Suspend { .. } => return Err(EvalErrorKind::UnsupportedOperation),
            }
        }
        for (index, temp) in function.temps.iter().enumerate() {
            meter.step()?;
            if temp.id.as_usize() != index {
                return Err(EvalErrorKind::InvalidIr);
            }
        }
        for (index, local) in function.locals.iter().enumerate() {
            meter.step()?;
            if local.id.as_usize() != index {
                return Err(EvalErrorKind::InvalidIr);
            }
        }
        for entry in &unit.literals.entries {
            // Literal decoding is bounded too: spellings can contain arbitrarily
            // many separators/leading zeros despite producing a small scalar.
            let AmirLiteralEntry::Int(text) = entry else {
                return Err(EvalErrorKind::InvalidLiteral);
            };
            for _ in text.bytes() {
                meter.step()?;
            }
        }
        let mut frame = Self {
            temps: slots(function.temps.len())?,
            locals: slots(function.locals.len())?,
            block: BlockId(0),
            instruction: 0,
            destination,
            unit,
        };
        for (&parameter, &argument) in frame.unit.function.params.iter().zip(args) {
            meter.step()?;
            let index = parameter.as_usize();
            let ty = frame
                .unit
                .temp_types
                .get(index)
                .ok_or(EvalErrorKind::InvalidIr)?;
            if !ty.accepts(argument) {
                return Err(EvalErrorKind::TypeMismatch);
            }
            *frame.temps.get_mut(index).ok_or(EvalErrorKind::InvalidIr)? = Some(argument);
        }
        meter.values += count;
        Ok(frame)
    }

    fn operand_type(&self, operand: AmirOperand) -> Result<Option<ScalarType>, EvalErrorKind> {
        match operand {
            AmirOperand::Copy(id) | AmirOperand::Move(id) => self
                .unit
                .temp_types
                .get(id.as_usize())
                .copied()
                .map(Some)
                .ok_or(EvalErrorKind::InvalidIr),
            AmirOperand::Constant(AmirConstant::Bool(_)) => Ok(Some(ScalarType::Bool)),
            AmirOperand::Constant(AmirConstant::Nil) => Ok(Some(ScalarType::Void)),
            AmirOperand::Constant(AmirConstant::Pool(_)) => Ok(None),
            AmirOperand::FunctionRef(_) | AmirOperand::GlobalRef(_) => {
                Err(EvalErrorKind::UnsupportedOperation)
            }
        }
    }

    fn operand<C: FnMut() -> bool>(
        &self,
        operand: AmirOperand,
        hint: Option<ScalarType>,
        meter: &mut Meter<'_, C>,
    ) -> Result<ConstValue, EvalErrorKind> {
        let value = match operand {
            // Every admitted scalar is Copy. Move here is a transfer of scalar
            // bits, not a simulation of owned runtime memory.
            AmirOperand::Copy(id) | AmirOperand::Move(id) => self
                .temps
                .get(id.as_usize())
                .ok_or(EvalErrorKind::InvalidIr)?
                .ok_or(EvalErrorKind::Uninitialized)?,
            AmirOperand::Constant(AmirConstant::Bool(value)) => ConstValue::Bool(value),
            AmirOperand::Constant(AmirConstant::Nil) => ConstValue::Void,
            AmirOperand::Constant(AmirConstant::Pool(id)) => {
                let index = usize::try_from(id.0).map_err(|_| EvalErrorKind::InvalidIr)?;
                let Some(AmirLiteralEntry::Int(text)) = self.unit.literals.entries.get(index)
                else {
                    return Err(EvalErrorKind::InvalidLiteral);
                };
                // Charge every decoding, not just admission: a loop can read
                // the same long spelling repeatedly. Bounding each parse keeps
                // work proportional to fuel rather than fuel × spelling size.
                for _ in text.bytes() {
                    meter.step()?;
                }
                let value = parse_int_literal(text).ok_or(EvalErrorKind::InvalidLiteral)?;
                let ty = match hint {
                    Some(ScalarType::Integer(ty)) => ty,
                    None => IntegerType::new(Primitive::Int, self.unit.layout)
                        .map_err(EvalErrorKind::Value)?,
                    _ => return Err(EvalErrorKind::TypeMismatch),
                };
                ConstValue::Integer(ConstInt::new(ty, value).map_err(EvalErrorKind::Value)?)
            }
            AmirOperand::FunctionRef(_) | AmirOperand::GlobalRef(_) => {
                return Err(EvalErrorKind::UnsupportedOperation);
            }
        };
        if hint.is_some_and(|ty| !ty.accepts(value)) {
            return Err(EvalErrorKind::TypeMismatch);
        }
        Ok(value)
    }

    fn assign(&mut self, id: TempId, value: ConstValue) -> Result<(), EvalErrorKind> {
        let ty = self
            .unit
            .temp_types
            .get(id.as_usize())
            .ok_or(EvalErrorKind::InvalidIr)?;
        if !ty.accepts(value) {
            return Err(EvalErrorKind::TypeMismatch);
        }
        *self
            .temps
            .get_mut(id.as_usize())
            .ok_or(EvalErrorKind::InvalidIr)? = Some(value);
        Ok(())
    }

    fn rvalue<C: FnMut() -> bool>(
        &self,
        rhs: &AmirRvalue,
        ty: ScalarType,
        meter: &mut Meter<'_, C>,
    ) -> Result<ConstValue, EvalErrorKind> {
        match rhs {
            AmirRvalue::Use(operand) => {
                // AMIR represents scalar coercions/casts as typed Use stores.
                // Convert a typed integer explicitly; pool literals instead
                // obtain their type from the destination at decoding time.
                if let (
                    ScalarType::Integer(destination),
                    AmirOperand::Copy(_) | AmirOperand::Move(_),
                ) = (ty, operand)
                {
                    let ConstValue::Integer(value) = self.operand(*operand, None, meter)? else {
                        return Err(EvalErrorKind::TypeMismatch);
                    };
                    value
                        .cast(destination)
                        .map(ConstValue::Integer)
                        .map_err(EvalErrorKind::Value)
                } else {
                    self.operand(*operand, Some(ty), meter)
                }
            }
            AmirRvalue::Unary { op, operand } => {
                eval_unary(*op, self.operand(*operand, Some(ty), meter)?)
                    .map_err(EvalErrorKind::Arithmetic)
            }
            AmirRvalue::Binary { op, left, right } => {
                let operand_ty =
                    self.operand_type(*left)?
                        .or(self.operand_type(*right)?)
                        .or(match ty {
                            ScalarType::Integer(_) => Some(ty),
                            _ => None,
                        });
                let a = self.operand(*left, operand_ty, meter)?;
                let right_hint = if matches!(
                    op,
                    arandu_middle::ops::BinaryOp::ShiftLeft
                        | arandu_middle::ops::BinaryOp::ShiftRight
                ) {
                    self.operand_type(*right)?.or(operand_ty)
                } else {
                    operand_ty
                };
                eval_binary(*op, a, self.operand(*right, right_hint, meter)?)
                    .map_err(EvalErrorKind::Arithmetic)
            }
            AmirRvalue::Load(place) if place.projections.is_empty() => self
                .locals
                .get(place.local.as_usize())
                .ok_or(EvalErrorKind::InvalidIr)?
                .ok_or(EvalErrorKind::Uninitialized),
            _ => Err(EvalErrorKind::UnsupportedOperation),
        }
    }

    fn jump<C: FnMut() -> bool>(
        &mut self,
        target: BlockId,
        args: &[AmirOperand],
        meter: &mut Meter<'_, C>,
    ) -> Result<(), EvalErrorKind> {
        let unit = Arc::clone(&self.unit);
        let block = unit
            .function
            .blocks
            .get(target.as_usize())
            .ok_or(EvalErrorKind::InvalidIr)?;
        let parameters = unit
            .function
            .block_params
            .get(slice_range(block.params)?)
            .ok_or(EvalErrorKind::InvalidIr)?;
        if args.len() != parameters.len() {
            return Err(EvalErrorKind::InvalidIr);
        }
        meter.reserve(args.len())?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(args.len())
            .map_err(|_| EvalErrorKind::AllocationFailed)?;
        for (&operand, parameter) in args.iter().zip(parameters) {
            meter.step()?;
            let ty = *self
                .unit
                .temp_types
                .get(parameter.id.as_usize())
                .ok_or(EvalErrorKind::InvalidIr)?;
            values.push(self.operand(operand, Some(ty), meter)?);
        }
        // Read all arguments before overwriting any block parameter (phi swap).
        for (parameter, value) in parameters.iter().zip(values) {
            self.assign(parameter.id, value)?;
        }
        self.block = target;
        self.instruction = 0;
        Ok(())
    }

    fn error(&self, kind: EvalErrorKind) -> EvalError {
        EvalError {
            kind,
            function: self.unit.function.symbol,
            block: self.block,
            span: self
                .unit
                .function
                .temps
                .first()
                .map_or(Span::new(0, 0, 0), |temp| temp.span),
        }
    }
}

enum Action {
    Continue,
    Call {
        unit: Arc<CtfeFunction>,
        values: Vec<ConstValue>,
        destination: Option<TempId>,
    },
    Return(ConstValue),
}

fn admitted_rvalue(rhs: &AmirRvalue) -> Result<(), EvalErrorKind> {
    match rhs {
        AmirRvalue::Use(_) => Ok(()),
        AmirRvalue::Unary {
            op: UnaryOp::Neg | UnaryOp::Not | UnaryOp::BitNot,
            ..
        } => Ok(()),
        AmirRvalue::Binary {
            op:
                BinaryOp::Add
                | BinaryOp::Sub
                | BinaryOp::Mul
                | BinaryOp::Div
                | BinaryOp::Mod
                | BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::Lt
                | BinaryOp::Gt
                | BinaryOp::LtEqual
                | BinaryOp::GtEqual
                | BinaryOp::And
                | BinaryOp::Or
                | BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::ShiftLeft
                | BinaryOp::ShiftRight,
            ..
        } => Ok(()),
        AmirRvalue::Load(place) if place.projections.is_empty() => Ok(()),
        AmirRvalue::FieldAccess { .. }
        | AmirRvalue::StructLiteral { .. }
        | AmirRvalue::IndexAccess { .. }
        | AmirRvalue::Array { .. }
        | AmirRvalue::Tuple { .. }
        | AmirRvalue::Discriminant { .. }
        | AmirRvalue::EnumPayload { .. }
        | AmirRvalue::EnumConstruct { .. }
        | AmirRvalue::Len(_)
        | AmirRvalue::SliceData(_)
        | AmirRvalue::SliceView { .. }
        | AmirRvalue::SliceSubslice { .. }
        | AmirRvalue::StrBytes { .. }
        | AmirRvalue::StrView { .. }
        | AmirRvalue::Alloc(_)
        | AmirRvalue::Load(_)
        | AmirRvalue::Borrow(_)
        | AmirRvalue::BorrowMut(_)
        | AmirRvalue::RelativeBorrow { .. }
        | AmirRvalue::CoroutineReady { .. }
        | AmirRvalue::GenInsert { .. }
        | AmirRvalue::GenGet { .. }
        | AmirRvalue::GenSet { .. }
        | AmirRvalue::GenUpsert { .. }
        | AmirRvalue::GenRemove { .. }
        | AmirRvalue::StringInterp { .. }
        | AmirRvalue::ToStr { .. }
        | AmirRvalue::BlackBox { .. }
        | AmirRvalue::Binary { .. }
        | AmirRvalue::Unary { .. } => Err(EvalErrorKind::UnsupportedOperation),
    }
}

/// Execute scalar AMIR with one shared fuel/value budget for the entire call
/// tree. Cancellation is polled during validation, instructions and transfers.
pub fn evaluate<P: FunctionProvider, C: FnMut() -> bool>(
    provider: &P,
    symbol: SymbolId,
    args: &[ConstValue],
    budget: Budget,
    mut cancelled: C,
) -> Result<ConstValue, EvalError> {
    let location = |kind| EvalError {
        kind,
        function: symbol,
        block: BlockId(0),
        span: Span::new(symbol.file_id, 0, 0),
    };
    let mut meter = Meter {
        fuel: budget.fuel,
        values: 0,
        max_values: usize::try_from(budget.values)
            .map_err(|_| location(EvalErrorKind::ValueLimit))?,
        max_frames: usize::try_from(budget.frames)
            .map_err(|_| location(EvalErrorKind::FrameLimit))?,
        cancelled: &mut cancelled,
    };
    meter.step().map_err(location)?;
    if meter.max_frames == 0 {
        return Err(location(EvalErrorKind::FrameLimit));
    }
    let unit = provider.function(symbol).map_err(location)?;
    if unit.function.symbol != symbol {
        return Err(location(EvalErrorKind::InvalidIr));
    }
    let layout = unit.layout;
    let root = Frame::new(unit, args, None, &mut meter).map_err(location)?;
    let mut stack = Vec::new();
    stack
        .try_reserve(1)
        .map_err(|_| location(EvalErrorKind::AllocationFailed))?;
    stack.push(root);
    loop {
        let depth = stack.len();
        let Some(frame) = stack.last_mut() else {
            return Err(location(EvalErrorKind::InvalidIr));
        };
        let step = (|| -> Result<Action, EvalErrorKind> {
            meter.step()?;
            let unit = Arc::clone(&frame.unit);
            let block = unit
                .function
                .blocks
                .get(frame.block.as_usize())
                .ok_or(EvalErrorKind::InvalidIr)?;
            let range = slice_range(block.statements)?;
            if frame.instruction < range.len() {
                let index = range
                    .start
                    .checked_add(frame.instruction)
                    .ok_or(EvalErrorKind::InvalidIr)?;
                let id = u32::try_from(index)
                    .map(InstrId)
                    .map_err(|_| EvalErrorKind::InvalidIr)?;
                let stmt = unit.function.try_stmt(id).ok_or(EvalErrorKind::InvalidIr)?;
                frame.instruction += 1;
                match stmt {
                    AmirStmt::Assign { lhs, rhs } => {
                        let ty = *frame
                            .unit
                            .temp_types
                            .get(lhs.as_usize())
                            .ok_or(EvalErrorKind::InvalidIr)?;
                        let value = frame.rvalue(rhs, ty, &mut meter)?;
                        frame.assign(*lhs, value)?;
                    }
                    AmirStmt::Store { lhs, rhs } => {
                        let index = lhs.local.as_usize();
                        let ty = *frame
                            .unit
                            .local_types
                            .get(index)
                            .ok_or(EvalErrorKind::InvalidIr)?;
                        let value = frame.operand(*rhs, Some(ty), &mut meter)?;
                        *frame
                            .locals
                            .get_mut(index)
                            .ok_or(EvalErrorKind::InvalidIr)? = Some(value);
                    }
                    AmirStmt::StorageLive(id) | AmirStmt::StorageDead(id) => {
                        *frame
                            .locals
                            .get_mut(id.as_usize())
                            .ok_or(EvalErrorKind::InvalidIr)? = None;
                    }
                    AmirStmt::Nop => {}
                    AmirStmt::Call {
                        lhs,
                        callee: AmirOperand::FunctionRef(callee),
                        args,
                        ..
                    } => {
                        if depth >= meter.max_frames {
                            return Err(EvalErrorKind::FrameLimit);
                        }
                        meter.reserve(args.len())?;
                        let callee_unit = provider.function(*callee)?;
                        if callee_unit.function.symbol != *callee {
                            return Err(EvalErrorKind::InvalidIr);
                        }
                        if callee_unit.layout != layout {
                            return Err(EvalErrorKind::TargetMismatch);
                        }
                        if callee_unit.function.params.len() != args.len() {
                            return Err(EvalErrorKind::InvalidIr);
                        }
                        let mut values = Vec::new();
                        values
                            .try_reserve_exact(args.len())
                            .map_err(|_| EvalErrorKind::AllocationFailed)?;
                        for (&operand, &parameter) in args.iter().zip(&callee_unit.function.params)
                        {
                            meter.step()?;
                            let ty = *callee_unit
                                .temp_types
                                .get(parameter.as_usize())
                                .ok_or(EvalErrorKind::InvalidIr)?;
                            values.push(frame.operand(operand, Some(ty), &mut meter)?);
                        }
                        return Ok(Action::Call {
                            unit: callee_unit,
                            values,
                            destination: *lhs,
                        });
                    }
                    AmirStmt::Call { .. } | AmirStmt::Free(_) | AmirStmt::Destroy(_) => {
                        return Err(EvalErrorKind::UnsupportedOperation);
                    }
                }
            } else {
                match &block.terminator {
                    AmirTerminator::Goto { target, args } => {
                        frame.jump(*target, args, &mut meter)?;
                    }
                    AmirTerminator::Branch {
                        condition,
                        if_true,
                        true_args,
                        if_false,
                        false_args,
                    } => {
                        let ConstValue::Bool(condition) =
                            frame.operand(*condition, Some(ScalarType::Bool), &mut meter)?
                        else {
                            return Err(EvalErrorKind::TypeMismatch);
                        };
                        let (target, args) = if condition {
                            (*if_true, true_args)
                        } else {
                            (*if_false, false_args)
                        };
                        frame.jump(target, args, &mut meter)?;
                    }
                    AmirTerminator::SwitchInt {
                        discriminant,
                        targets,
                        otherwise,
                    } => {
                        let ConstValue::Integer(value) =
                            frame.operand(*discriminant, None, &mut meter)?
                        else {
                            return Err(EvalErrorKind::TypeMismatch);
                        };
                        let mut selected = (&otherwise.0, &otherwise.1);
                        for (tag, target, args) in targets {
                            meter.step()?;
                            if *tag == value.value() {
                                selected = (target, args);
                                break;
                            }
                        }
                        let (target, args) = selected;
                        frame.jump(*target, args, &mut meter)?;
                    }
                    AmirTerminator::Return => {
                        let value = if unit.return_type == ScalarType::Void {
                            ConstValue::Void
                        } else {
                            frame.operand(
                                AmirOperand::Copy(TempId(0)),
                                Some(unit.return_type),
                                &mut meter,
                            )?
                        };
                        return Ok(Action::Return(value));
                    }
                    AmirTerminator::Unreachable => return Err(EvalErrorKind::InvalidIr),
                    AmirTerminator::Suspend { .. } => {
                        return Err(EvalErrorKind::UnsupportedOperation);
                    }
                }
            }
            Ok(Action::Continue)
        })();
        let action = step.map_err(|kind| frame.error(kind))?;
        match action {
            Action::Continue => {}
            Action::Call {
                unit,
                values,
                destination,
            } => {
                meter.values += values.len();
                let child = Frame::new(unit, &values, destination, &mut meter)
                    .map_err(|kind| frame.error(kind))?;
                meter.values -= values.len();
                let error = frame.error(EvalErrorKind::AllocationFailed);
                stack.try_reserve(1).map_err(|_| error)?;
                stack.push(child);
            }
            Action::Return(value) => {
                let destination = frame.destination;
                let count = frame.temps.len() + frame.locals.len();
                stack.pop();
                meter.values -= count;
                if let Some(parent) = stack.last_mut() {
                    if let Some(destination) = destination {
                        parent
                            .assign(destination, value)
                            .map_err(|kind| parent.error(kind))?;
                    }
                } else {
                    return Ok(value);
                }
            }
        }
    }
}
