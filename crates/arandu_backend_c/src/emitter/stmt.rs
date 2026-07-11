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
                // A3.3 multi-stmt stack: declare payload local, store value, take address.
                // (Statement-expression `&local` would dangle after the expression ends.)
                if let arandu_middle::amir::AmirRvalue::CoroutineReady {
                    value,
                    payload_ty,
                    stack: true,
                } = rhs
                {
                    // A3.6 stack blob: disc@0 + payload@8 (aligned buffer).
                    let payload_ar = self.interner.resolve(*payload_ty);
                    let payload_c = self.format_type(&payload_ar);
                    let v = self.format_operand(value, func);
                    let slot = self.next_co_stack_slot();
                    let payload_size = self
                        .layout
                        .layout_of_type(&payload_ar, self.interner, self.provider)
                        .size
                        .max(1);
                    let size = 8 + payload_size;
                    let _ = writeln!(
                        &mut self.output,
                        "    uint8_t __ar_co_{slot}[{size}] __attribute__((aligned(8)));"
                    );
                    let _ = writeln!(&mut self.output, "    *(uint32_t*)__ar_co_{slot} = 0;");
                    let _ = writeln!(
                        &mut self.output,
                        "    *({payload_c}*)(__ar_co_{slot} + 8) = ({payload_c})({v});"
                    );
                    let _ = writeln!(
                        &mut self.output,
                        "    t{} = (void*)__ar_co_{slot};",
                        lhs.as_usize()
                    );
                    return;
                }
                let _ = write!(&mut self.output, "    t{} = ", lhs.as_usize());
                self.emit_rvalue(rhs, func, &lhs_ty, &lhs_c_ty);
                let _ = writeln!(&mut self.output, ";");
            }
            AmirStmt::Store { lhs, rhs } => {
                let lhs_str = self.format_place(lhs, func);
                let rhs_str = self.format_operand(rhs, func);
                let _ = writeln!(&mut self.output, "    {} = {};", lhs_str, rhs_str);
            }
            AmirStmt::Call { lhs, callee, args } => {
                let callee_str = self.format_operand(callee, func);
                let args_str: Vec<_> = args.iter().map(|a| self.format_operand(a, func)).collect();
                if let Some(dest) = lhs {
                    let _ = write!(&mut self.output, "    t{} = ", dest.as_usize());
                } else {
                    let _ = write!(&mut self.output, "    ");
                }
                let _ = write!(&mut self.output, "{callee_str}(");
                for (i, arg_str) in args_str.iter().enumerate() {
                    if i > 0 {
                        let _ = write!(&mut self.output, ", ");
                    }
                    let _ = write!(&mut self.output, "{}", arg_str);
                }
                let _ = writeln!(&mut self.output, ");");
            }
            AmirStmt::Free(op) => {
                let op_str = self.format_operand(op, func);
                let _ = writeln!(&mut self.output, "    free({});", op_str);
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
                        let _ = writeln!(&mut self.output, "    return 0;");
                    } else {
                        let _ = writeln!(&mut self.output, "    return (int)t0;");
                    }
                } else if matches!(ret, ArType::Void) {
                    let _ = writeln!(&mut self.output, "    return;");
                } else {
                    let _ = writeln!(&mut self.output, "    return t0;");
                }
            }
            AmirTerminator::Goto { target, args } => {
                self.emit_block_arguments(*target, args, func, "    ");
                let _ = writeln!(&mut self.output, "    goto bb{};", target.as_usize());
            }
            // A3.1 ready-only: suspend = jump to resume (await load is in resume BB).
            AmirTerminator::Suspend {
                future: _,
                resume,
                args,
            } => {
                self.emit_block_arguments(*resume, args, func, "    ");
                let _ = writeln!(&mut self.output, "    goto bb{};", resume.as_usize());
            }
            AmirTerminator::Branch {
                condition,
                if_true,
                true_args,
                if_false,
                false_args,
            } => {
                let cond_str = self.format_operand(condition, func);
                let _ = writeln!(&mut self.output, "    if ({}) {{", cond_str);
                self.emit_block_arguments(*if_true, true_args, func, "        ");
                let _ = writeln!(&mut self.output, "        goto bb{};", if_true.as_usize());
                let _ = writeln!(&mut self.output, "    }} else {{");
                self.emit_block_arguments(*if_false, false_args, func, "        ");
                let _ = writeln!(&mut self.output, "        goto bb{};", if_false.as_usize());
                let _ = writeln!(&mut self.output, "    }}");
            }
            AmirTerminator::SwitchInt {
                discriminant,
                targets,
                otherwise,
            } => {
                let discr_str = self.format_operand(discriminant, func);
                let _ = writeln!(&mut self.output, "    switch ({}) {{", discr_str);
                for (val, target, args) in targets.iter() {
                    let _ = writeln!(&mut self.output, "        case {}:", val);
                    self.emit_block_arguments(*target, args, func, "            ");
                    let _ = writeln!(
                        &mut self.output,
                        "            goto bb{};",
                        target.as_usize()
                    );
                }
                let _ = writeln!(&mut self.output, "        default:");
                self.emit_block_arguments(otherwise.0, &otherwise.1, func, "            ");
                let _ = writeln!(
                    &mut self.output,
                    "            goto bb{};",
                    otherwise.0.as_usize()
                );
                let _ = writeln!(&mut self.output, "    }}");
            }
            AmirTerminator::Unreachable => {
                let _ = writeln!(&mut self.output, "    AR_UNREACHABLE();");
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
            let _ = writeln!(
                &mut self.output,
                "{}t{} = {};",
                indent,
                param.id.as_usize(),
                arg_str
            );
        }
    }
}
