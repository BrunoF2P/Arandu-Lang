use super::CEmitter;
use arandu_middle::amir::{AmirFunc, AmirOperand, AmirStmt, AmirTerminator};
use arandu_middle::types::ArType;
use std::fmt::Write;

impl<'a> CEmitter<'a> {
    pub(super) fn emit_stmt(&mut self, stmt: &AmirStmt, func: &AmirFunc) {
        match stmt {
            AmirStmt::Assign { lhs, rhs } => {
                let lhs_ty = self.temp_ty(func, *lhs);
                let lhs_c_ty = self.format_type(&lhs_ty);
                write!(&mut self.output, "    t{} = ", lhs.as_usize()).unwrap();
                self.emit_rvalue(rhs, func, &lhs_ty, &lhs_c_ty);
                writeln!(&mut self.output, ";").unwrap();
            }
            AmirStmt::Store { lhs, rhs } => {
                let lhs_str = self.format_place(lhs, func);
                let rhs_str = self.format_operand(rhs, func);
                writeln!(&mut self.output, "    {} = {};", lhs_str, rhs_str).unwrap();
            }
            AmirStmt::Call { lhs, callee, args } => {
                let callee_str = self.format_operand(callee, func);
                let args_str: Vec<_> = args.iter().map(|a| self.format_operand(a, func)).collect();
                if let Some(dest) = lhs {
                    write!(&mut self.output, "    t{} = ", dest.as_usize()).unwrap();
                } else {
                    write!(&mut self.output, "    ").unwrap();
                }
                write!(&mut self.output, "{callee_str}(").unwrap();
                for (i, arg_str) in args_str.iter().enumerate() {
                    if i > 0 {
                        write!(&mut self.output, ", ").unwrap();
                    }
                    write!(&mut self.output, "{}", arg_str).unwrap();
                }
                writeln!(&mut self.output, ");").unwrap();
            }
            AmirStmt::Free(op) => {
                let op_str = self.format_operand(op, func);
                writeln!(&mut self.output, "    free({});", op_str).unwrap();
            }
            AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) => {}
            AmirStmt::Destroy(_) | AmirStmt::Nop => {}
        }
    }

    pub(super) fn emit_terminator(&mut self, term: &AmirTerminator, func: &AmirFunc) {
        match term {
            AmirTerminator::Return => {
                let name = super::sanitize_c_ident(&self.symbols.get(func.symbol).name);
                let ret = self.interner.resolve(func.return_type);
                if name == "main" {
                    // ISO C requires `int main`; void Arandu main becomes `return 0`.
                    if matches!(ret, ArType::Void) {
                        writeln!(&mut self.output, "    return 0;").unwrap();
                    } else {
                        writeln!(&mut self.output, "    return (int)t0;").unwrap();
                    }
                } else if matches!(ret, ArType::Void) {
                    writeln!(&mut self.output, "    return;").unwrap();
                } else {
                    writeln!(&mut self.output, "    return t0;").unwrap();
                }
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

    pub(super) fn emit_block_arguments(
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
}
