use crate::ast::{BitwiseUnaryOp, CompareOp, ExprOp, MathOp};
use crate::backend::{BackendError, RuntimeEmitter};
use crate::ir;
use std::collections::HashMap;

use super::asm;

#[derive(Clone, Copy)]
enum StackSlotKind {
    Scalar(crate::ast::MemoryWidth),
    String,
}

pub(crate) fn emit_for_target_with_entry(
    program: &ir::Program,
    target: crate::backend::Target,
    entry_symbol: &str,
) -> Result<String, BackendError> {
    const FRAME_PREFIX: usize = 48;
    let mut asm = String::new();
    emit_data(&mut asm, program, entry_symbol)?;
    asm::text(&mut asm);
    asm::global(&mut asm, entry_symbol);
    asm.push('\n');

    for label in &program.labels {
        let slots = stack_slots(&label.stack, FRAME_PREFIX);
        let slot_kinds = stack_slot_kinds(&label.stack);
        let frame_size = stack_frame_size(&label.stack) + FRAME_PREFIX;
        let constants = label
            .instructions
            .iter()
            .filter_map(|instruction| match instruction {
                ir::Instruction::Const { name, value } => Some((name.clone(), value.clone())),
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        asm::label(
            &mut asm,
            if label.name == program.entry {
                entry_symbol
            } else {
                &label.name
            },
        );
        asm::instruction(
            &mut asm,
            format_args!(
                "sub sp, sp, #{frame_size}\n  stp x29, x30, [sp]\n  mov x29, sp\n  stp x19, x20, [sp, #16]\n  str x21, [sp, #32]"
            ),
        );
        for (index, instruction) in label.instructions.iter().enumerate() {
            emit_instruction(
                &mut asm,
                instruction,
                &slots,
                &slot_kinds,
                &constants,
                frame_size,
                target,
            )
            .map_err(|message| BackendError::new(message).at(&label.name, index))?;
        }
        if !label
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, ir::Instruction::Ret))
        {
            asm::instruction(
                &mut asm,
                &format!(
                    "ldp x19, x20, [sp, #16]\n  ldr x21, [sp, #32]\n  ldp x29, x30, [sp]\n  add sp, sp, #{frame_size}"
                ),
            );
        }
    }

    Ok(asm)
}

fn emit_data(asm: &mut String, program: &ir::Program, entry_symbol: &str) -> Result<(), String> {
    if !program.data.is_empty() {
        for declaration in &program.data {
            if declaration.keep {
                asm::top_level_directive(
                    asm,
                    format_args!(".section .{}, \"aR\", @progbits", declaration.section),
                );
            } else {
                asm::section(asm, &declaration.section);
            }
            if let Some(align) = declaration.align {
                asm::top_level_directive(asm, format_args!(".balign {align}"));
            }
            if declaration.export {
                asm::global(asm, &declaration.name);
            }
            asm::label(asm, &declaration.name);
            for item in &declaration.items {
                match item {
                    ir::DataItem::Scalar { width, value } => {
                        asm::directive(asm, format_args!("{} {value}", data_directive(*width)?));
                    }
                    ir::DataItem::Address { target } => asm::directive(
                        asm,
                        format_args!(".quad {}", remap_entry(target, program, entry_symbol)),
                    ),
                    ir::DataItem::Zero { count } => {
                        asm::directive(asm, format_args!(".zero {count}"))
                    }
                    ir::DataItem::Label { name } => asm::label(asm, name),
                }
            }
        }
    }

    for memory in &program.memory {
        match memory {
            ir::MemoryDeclaration::Buffer { name, width, count } => {
                asm::section(asm, "bss");
                asm::label(asm, name);
                asm::directive(asm, format_args!(".zero {}", width_size(*width) * count));
            }
            ir::MemoryDeclaration::Scalar { name, width, value } => {
                asm::section(asm, "data");
                asm::label(asm, name);
                asm::directive(asm, format_args!("{} {value}", data_directive(*width)?));
            }
            ir::MemoryDeclaration::FloatScalar { name, width, value } => {
                asm::section(asm, "data");
                asm::label(asm, name);
                asm::directive(asm, format_args!("{} {value}", data_directive(*width)?));
            }
            ir::MemoryDeclaration::Array {
                name,
                width,
                values,
            } => {
                asm::section(asm, "data");
                asm::label(asm, name);
                for value in values {
                    emit_memory_value(asm, *width, value, program, entry_symbol)?;
                }
            }
            ir::MemoryDeclaration::Repeat {
                name,
                width,
                count,
                value,
            } => {
                asm::section(asm, "data");
                asm::label(asm, name);
                for _ in 0..*count {
                    emit_memory_value(asm, *width, value, program, entry_symbol)?;
                }
            }
        }
    }
    Ok(())
}

fn emit_memory_value(
    asm: &mut String,
    width: crate::ast::MemoryWidth,
    value: &ir::MemoryValue,
    program: &ir::Program,
    entry_symbol: &str,
) -> Result<(), String> {
    match value {
        ir::MemoryValue::Integer(value) => {
            asm::directive(asm, format_args!("{} {value}", data_directive(width)?))
        }
        ir::MemoryValue::Address { target } => asm::directive(
            asm,
            format_args!(".quad {}", remap_entry(target, program, entry_symbol)),
        ),
    }
    Ok(())
}

fn remap_entry(target: &str, program: &ir::Program, entry_symbol: &str) -> String {
    if target == program.entry {
        entry_symbol.to_owned()
    } else {
        target.to_owned()
    }
}

fn data_directive(width: crate::ast::MemoryWidth) -> Result<&'static str, String> {
    match width {
        crate::ast::MemoryWidth::I8 | crate::ast::MemoryWidth::U8 => Ok(".byte"),
        crate::ast::MemoryWidth::I16 | crate::ast::MemoryWidth::U16 => Ok(".hword"),
        crate::ast::MemoryWidth::I32 | crate::ast::MemoryWidth::U32 => Ok(".word"),
        crate::ast::MemoryWidth::I64
        | crate::ast::MemoryWidth::U64
        | crate::ast::MemoryWidth::Ptr => Ok(".quad"),
        crate::ast::MemoryWidth::F32 => Ok(".float"),
        crate::ast::MemoryWidth::F64 => Ok(".double"),
    }
}

fn width_size(width: crate::ast::MemoryWidth) -> usize {
    match width {
        crate::ast::MemoryWidth::I8 | crate::ast::MemoryWidth::U8 => 1,
        crate::ast::MemoryWidth::I16 | crate::ast::MemoryWidth::U16 => 2,
        crate::ast::MemoryWidth::I32
        | crate::ast::MemoryWidth::U32
        | crate::ast::MemoryWidth::F32 => 4,
        _ => 8,
    }
}

fn emit_instruction(
    asm: &mut String,
    instruction: &ir::Instruction,
    slots: &HashMap<String, usize>,
    slot_kinds: &HashMap<String, StackSlotKind>,
    constants: &HashMap<String, ir::ConstValue>,
    frame_size: usize,
    target: crate::backend::Target,
) -> Result<(), String> {
    match instruction {
        ir::Instruction::Assign { dst, value } => {
            emit_assignment(asm, dst, value, slots, slot_kinds, constants, target)
        }
        ir::Instruction::PairAssign { dst, op, lhs, rhs } => {
            validate_aarch64_pair_register("Pair arithmetic destination high register", &dst.high)?;
            validate_aarch64_pair_register("Pair arithmetic destination low register", &dst.low)?;
            validate_aarch64_pair_register("Pair arithmetic right high register", &rhs.high)?;
            validate_aarch64_pair_register("Pair arithmetic right low register", &rhs.low)?;
            if dst != lhs {
                return Err(String::from(
                    "AArch64 pair arithmetic destination must match the left operand pair",
                ));
            }
            if dst.high == dst.low {
                return Err(String::from(
                    "AArch64 pair arithmetic destination registers must differ",
                ));
            }
            if rhs.high == dst.low {
                return Err(String::from(
                    "AArch64 pair arithmetic right high register cannot overlap destination low register",
                ));
            }
            let (first, second) = match op {
                crate::ast::PairBinaryOp::Add => ("adds", "adc"),
                crate::ast::PairBinaryOp::Subtract => ("subs", "sbc"),
            };
            asm::instruction(
                asm,
                format_args!(
                    "{first} {}, {}, {}\n  {second} {}, {}, {}",
                    dst.low, lhs.low, rhs.low, dst.high, lhs.high, rhs.high
                ),
            );
            Ok(())
        }
        ir::Instruction::WideAssign {
            dst,
            signed,
            division,
            lhs,
            rhs,
        } => {
            if dst.high != "x1" || dst.low != "x0" {
                return Err(format!(
                    "AArch64 widened math destination must be x1:x0, found {}:{}",
                    dst.high, dst.low
                ));
            }
            if operand_uses_register(lhs, "x16")
                || operand_uses_register(lhs, "x17")
                || operand_uses_register(rhs, "x16")
                || operand_uses_register(rhs, "x17")
            {
                return Err(String::from(
                    "AArch64 widened math operands cannot use x16 or x17 scratch registers",
                ));
            }
            emit_value(asm, "x16", lhs, slots)?;
            emit_value(asm, "x17", rhs, slots)?;
            if *division {
                asm::instruction(
                    asm,
                    format_args!(
                        "{} x0, x16, x17\n  msub x1, x0, x17, x16",
                        if *signed { "sdiv" } else { "udiv" }
                    ),
                );
            } else {
                asm::instruction(
                    asm,
                    format_args!(
                        "mul x0, x16, x17\n  {} x1, x16, x17",
                        if *signed { "smulh" } else { "umulh" }
                    ),
                );
            }
            Ok(())
        }
        ir::Instruction::AssignIf {
            dst,
            value,
            condition,
        } => {
            let skip = format!(".L.__subsea.aarch64.assign_if_skip_{}", asm.len());
            emit_condition_branch(asm, condition, &skip, false, slots)?;
            emit_assignment(asm, dst, value, slots, slot_kinds, constants, target)?;
            asm::label(asm, &skip);
            Ok(())
        }
        ir::Instruction::Call { target } => {
            match target {
                ir::ControlTarget::Label(target) => asm::call(asm, target),
                ir::ControlTarget::Operand(target) => {
                    validate_control_target(target, slot_kinds, "call")?;
                    emit_value(asm, "x16", target, slots)?;
                    asm::call_register(asm, "x16");
                }
            }
            Ok(())
        }
        ir::Instruction::Exit { code } => {
            if !target.supports_runtime(crate::backend::RuntimeOperation::Exit) {
                return unsupported("linux.exit on freestanding target");
            }
            AArch64RuntimeEmitter {
                slots,
                slot_kinds,
                constants,
            }
            .emit_exit(asm, *code)
            .map_err(|error| error.message)
        }
        ir::Instruction::Jmp { target, condition } => {
            if let Some(condition) = condition {
                let skip = format!(".L.__subsea.aarch64.jmp_skip_{}", asm.len());
                emit_condition_branch(asm, condition, &skip, false, slots)?;
                emit_control_target(asm, target, slots, slot_kinds)?;
                asm::label(asm, &skip);
                Ok(())
            } else {
                emit_control_target(asm, target, slots, slot_kinds)
            }
        }
        ir::Instruction::Label { name } => {
            asm::label(asm, name);
            Ok(())
        }
        ir::Instruction::Nop => {
            asm::instruction(asm, "nop");
            Ok(())
        }
        ir::Instruction::Runtime(operation) => {
            let runtime = match operation {
                ir::RuntimeOperation::Print { .. } => crate::backend::RuntimeOperation::Write,
                ir::RuntimeOperation::Read { .. } => crate::backend::RuntimeOperation::Read,
                ir::RuntimeOperation::Release { .. } => crate::backend::RuntimeOperation::Release,
            };
            if !target.supports_runtime(runtime) {
                return unsupported("Linux runtime operation on freestanding target");
            }
            AArch64RuntimeEmitter {
                slots,
                slot_kinds,
                constants,
            }
            .emit_runtime(asm, operation)
            .map_err(|error| error.message)
        }
        ir::Instruction::Ret => {
            asm::instruction(
                asm,
                format_args!(
                    "ldp x19, x20, [sp, #16]\n  ldr x21, [sp, #32]\n  ldp x29, x30, [sp]\n  add sp, sp, #{frame_size}"
                ),
            );
            asm::ret(asm);
            Ok(())
        }
        ir::Instruction::Stack { name, width, value } => {
            let dst = stack_operand(name, Some(*width), slots)?;
            emit_assignment(
                asm,
                &dst,
                &ir::Value::Operand(value.clone()),
                slots,
                slot_kinds,
                constants,
                target,
            )
        }
        ir::Instruction::Const { .. } => Ok(()),
        ir::Instruction::StackString { name, value } => emit_stack_string(asm, name, value, slots),
        ir::Instruction::Push { src } => {
            validate_stack_value(src, slots, slot_kinds, "push")?;
            emit_value(asm, "x16", src, slots)?;
            asm::instruction(asm, "str x16, [sp, #-16]!");
            Ok(())
        }
        ir::Instruction::Pop { dst } => match dst {
            ir::Operand::TargetRegister(dst) if is_aarch64_x_register(dst) => {
                asm::load(asm, "ldr", dst, "[sp], #16");
                Ok(())
            }
            ir::Operand::Memory {
                address,
                width:
                    Some(
                        crate::ast::MemoryWidth::I64
                        | crate::ast::MemoryWidth::U64
                        | crate::ast::MemoryWidth::Ptr,
                    ),
            } => {
                asm::load(asm, "ldr", "x16", "[sp], #16");
                let address = memory_address_or_materialize(asm, address)?;
                asm::store(asm, "str", "x16", address);
                Ok(())
            }
            _ => unsupported(
                "pop destination must be a 64-bit register or explicitly 64-bit memory operand",
            ),
        },
        ir::Instruction::Syscall => {
            asm::svc(asm);
            Ok(())
        }
        ir::Instruction::InlineAsm {
            architecture: crate::ast::InlineAsmArchitecture::AArch64,
            text,
        } => {
            asm::instruction(asm, text);
            Ok(())
        }
        ir::Instruction::InlineAsm { .. } => unsupported("inline assembly architecture"),
    }
}

fn validate_stack_value(
    operand: &ir::Operand,
    slots: &HashMap<String, usize>,
    slot_kinds: &HashMap<String, StackSlotKind>,
    instruction: &str,
) -> Result<(), String> {
    let valid = match operand {
        ir::Operand::Immediate(_) => true,
        ir::Operand::TargetRegister(register) => is_aarch64_x_register(register),
        ir::Operand::Memory { width, .. } => width.is_some_and(|width| {
            matches!(
                width,
                crate::ast::MemoryWidth::I64
                    | crate::ast::MemoryWidth::U64
                    | crate::ast::MemoryWidth::Ptr
            )
        }),
        ir::Operand::Name(name) => {
            matches!(
                slot_kinds.get(name),
                Some(StackSlotKind::Scalar(
                    crate::ast::MemoryWidth::I64
                        | crate::ast::MemoryWidth::U64
                        | crate::ast::MemoryWidth::Ptr,
                ))
            ) && slots.contains_key(name)
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "AArch64 {instruction} operand must be a 64-bit integer value"
        ))
    }
}

struct AArch64RuntimeEmitter<'a> {
    slots: &'a HashMap<String, usize>,
    slot_kinds: &'a HashMap<String, StackSlotKind>,
    constants: &'a HashMap<String, ir::ConstValue>,
}

impl RuntimeEmitter for AArch64RuntimeEmitter<'_> {
    fn emit_runtime(
        &mut self,
        asm: &mut String,
        operation: &ir::RuntimeOperation,
    ) -> Result<(), BackendError> {
        emit_runtime_operation(asm, operation, self.slots, self.slot_kinds, self.constants)
            .map_err(BackendError::from)
    }

    fn emit_exit(&mut self, asm: &mut String, code: u8) -> Result<(), BackendError> {
        asm::instruction(asm, format_args!("mov x0, #{code}\n  mov x8, #93"));
        asm::svc(asm);
        Ok(())
    }

    fn emit_reserve(
        &mut self,
        asm: &mut String,
        dst: &ir::Operand,
        len: &ir::Operand,
    ) -> Result<(), BackendError> {
        emit_linux_reserve(asm, dst, len, self.slots, self.slot_kinds).map_err(BackendError::from)
    }
}

fn emit_runtime_operation(
    asm: &mut String,
    operation: &ir::RuntimeOperation,
    slots: &HashMap<String, usize>,
    slot_kinds: &HashMap<String, StackSlotKind>,
    constants: &HashMap<String, ir::ConstValue>,
) -> Result<(), String> {
    match operation {
        ir::RuntimeOperation::Print { parts } => {
            for part in parts {
                match part {
                    ir::PrintPart::Literal(value) => emit_literal_write(asm, value),
                    ir::PrintPart::Binding(name) => {
                        if let Some(offset) = slots.get(name) {
                            match slot_kinds.get(name) {
                                Some(StackSlotKind::String) => {
                                    asm::mov(asm, "x0", "#1");
                                    asm::load(asm, "ldr", "x1", format_args!("[x29, #{offset}]"));
                                    asm::load(
                                        asm,
                                        "ldr",
                                        "x2",
                                        format_args!("[x29, #{}]", offset + 8),
                                    );
                                    asm::mov(asm, "x8", "#64");
                                    asm::svc(asm);
                                }
                                Some(StackSlotKind::Scalar(width)) => emit_integer_print(
                                    asm,
                                    &ir::Operand::Name(name.clone()),
                                    print_format_for_width(*width),
                                    slots,
                                    slot_kinds,
                                )?,
                                None => return Err(format!("Unknown print binding {name:?}")),
                            }
                        } else {
                            match constants.get(name) {
                                Some(ir::ConstValue::String(value)) => {
                                    emit_literal_write(asm, value)
                                }
                                Some(ir::ConstValue::Integer { value, width }) => {
                                    emit_integer_print(
                                        asm,
                                        &ir::Operand::Immediate(*value),
                                        print_format_for_width(
                                            width.unwrap_or(crate::ast::MemoryWidth::I64),
                                        ),
                                        slots,
                                        slot_kinds,
                                    )?;
                                }
                                Some(ir::ConstValue::Float { value, .. }) => {
                                    emit_literal_write(asm, value);
                                }
                                None => return Err(format!("Unknown print binding {name:?}")),
                            }
                        }
                    }
                    ir::PrintPart::Operand(operand) => {
                        if let Some(format) = const_string_property_format(operand, constants) {
                            emit_const_string_property(asm, operand, format, constants)?;
                            continue;
                        }
                        emit_integer_print(
                            asm,
                            operand,
                            ir::PrintFormat::SignedDecimal(crate::ast::MemoryWidth::I64),
                            slots,
                            slot_kinds,
                        )?;
                    }
                    ir::PrintPart::FormattedOperand { format, operand } => {
                        if matches!(format, ir::PrintFormat::Infer)
                            && const_string_property_format(operand, constants).is_some()
                        {
                            emit_const_string_property(
                                asm,
                                operand,
                                const_string_property_format(operand, constants).unwrap(),
                                constants,
                            )?;
                            continue;
                        }
                        emit_integer_print(asm, operand, *format, slots, slot_kinds)?;
                    }
                }
            }
            Ok(())
        }
        ir::RuntimeOperation::Read {
            source: ir::ReadSource::Stdin,
            dst,
            len,
        } => {
            validate_read_destination(dst, slots, slot_kinds)?;
            validate_linux_size_operand(len, slots, slot_kinds, "read length")?;
            asm::mov(asm, "x0", "#0");
            emit_address_or_value(asm, "x1", dst, slots)?;
            emit_value(asm, "x2", len, slots)?;
            asm::mov(asm, "x8", "#63");
            asm::svc(asm);
            Ok(())
        }
        ir::RuntimeOperation::Release { ptr, len } => {
            validate_linux_pointer_operand(ptr, slots, slot_kinds, "release pointer")?;
            validate_linux_size_operand(len, slots, slot_kinds, "release size")?;
            emit_value(asm, "x0", ptr, slots)?;
            emit_value(asm, "x1", len, slots)?;
            asm::mov(asm, "x8", "#215");
            asm::svc(asm);
            Ok(())
        }
    }
}

fn const_string_property_format(
    operand: &ir::Operand,
    constants: &HashMap<String, ir::ConstValue>,
) -> Option<ir::PrintFormat> {
    let ir::Operand::StringProperty { name, property } = operand else {
        return None;
    };
    if !matches!(constants.get(name), Some(ir::ConstValue::String(_))) {
        return None;
    }
    Some(match property {
        ir::StringProperty::Len => ir::PrintFormat::UnsignedDecimal(crate::ast::MemoryWidth::U64),
        ir::StringProperty::Ptr => ir::PrintFormat::Pointer,
    })
}

fn emit_const_string_property(
    asm: &mut String,
    operand: &ir::Operand,
    format: ir::PrintFormat,
    constants: &HashMap<String, ir::ConstValue>,
) -> Result<(), String> {
    let ir::Operand::StringProperty { name, property } = operand else {
        return Err(String::from("expected string property"));
    };
    if matches!(property, ir::StringProperty::Len) {
        let Some(ir::ConstValue::String(value)) = constants.get(name) else {
            return Err(format!("Unknown string constant {name:?}"));
        };
        return emit_integer_print(
            asm,
            &ir::Operand::Immediate(value.len() as i128),
            format,
            &HashMap::new(),
            &HashMap::new(),
        );
    }
    let label = format!(".L.__subsea.aarch64.const_string_{}", asm.len());
    let Some(ir::ConstValue::String(value)) = constants.get(name) else {
        return Err(format!("Unknown string constant {name:?}"));
    };
    asm::section(asm, "rodata");
    asm::label(asm, &label);
    let bytes = if value.is_empty() {
        String::from("0")
    } else {
        value
            .as_bytes()
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    asm::directive(asm, format_args!(".byte {bytes}"));
    asm::text(asm);
    emit_integer_print(
        asm,
        &ir::Operand::Pointer(label),
        format,
        &HashMap::new(),
        &HashMap::new(),
    )
}

fn emit_literal_write(asm: &mut String, value: &str) {
    let label = format!(".L.__subsea.aarch64.string_{}", asm.len());
    let bytes = value
        .as_bytes()
        .iter()
        .map(|byte| byte.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    asm::section(asm, "rodata");
    asm::label(asm, &label);
    asm::instruction(asm, format_args!(".byte {bytes}"));
    asm::text(asm);
    asm::instruction(
        asm,
        format_args!(
            "mov x0, #1\n  adrp x1, {label}\n  add x1, x1, :lo12:{label}\n  mov x2, #{}\n  mov x8, #64",
            value.len()
        ),
    );
    asm::svc(asm);
}

fn emit_integer_print(
    asm: &mut String,
    source: &ir::Operand,
    format: ir::PrintFormat,
    slots: &HashMap<String, usize>,
    slot_kinds: &HashMap<String, StackSlotKind>,
) -> Result<(), String> {
    let format = if matches!(format, ir::PrintFormat::Infer) {
        infer_print_format(source, slot_kinds)?
    } else {
        format
    };
    let signed = matches!(format, ir::PrintFormat::SignedDecimal(_));
    let (base, prefix) = match format {
        ir::PrintFormat::SignedDecimal(_) | ir::PrintFormat::UnsignedDecimal(_) => (10, ""),
        ir::PrintFormat::Hex => (16, "0x"),
        ir::PrintFormat::Binary => (2, "0b"),
        ir::PrintFormat::Pointer => (16, "0x"),
        ir::PrintFormat::Infer => return unsupported("inferred runtime printing"),
    };
    if let ir::Operand::TargetRegister(register) = source {
        if register.starts_with('w') {
            asm::mov(asm, "w16", register);
        } else {
            emit_value(asm, "x16", source, slots)?;
        }
    } else {
        emit_value(asm, "x16", source, slots)?;
    }
    normalize_print_width(asm, format)?;
    let id = asm.len();
    let loop_label = format!(".L.__subsea.aarch64.print_loop_{id}");
    let done_label = format!(".L.__subsea.aarch64.print_done_{id}");
    let zero_label = format!(".L.__subsea.aarch64.print_zero_{id}");
    let sign_label = format!(".L.__subsea.aarch64.print_sign_{id}");
    let buffer = format!(".L.__subsea.aarch64.print_buffer_{id}");
    asm::section(asm, "bss");
    asm::label(asm, &buffer);
    asm::instruction(asm, ".zero 128");
    asm::text(asm);
    asm::instruction(
        asm,
        format_args!(
            "adrp x17, {buffer}\n  add x17, x17, :lo12:{buffer}\n  add x17, x17, #128\n  mov x18, #{base}\n  cbz x16, {zero_label}"
        ),
    );
    if signed {
        asm::instruction(asm, format_args!("tbnz x16, #63, {sign_label}"));
    }
    if base == 10 {
        asm::label(asm, &loop_label);
        asm::instruction(
            asm,
            format_args!(
                "udiv x19, x16, x18\n  msub x20, x19, x18, x16\n  add x20, x20, #48\n  strb w20, [x17, #-1]!\n  mov x16, x19\n  cbnz x16, {loop_label}\n  b {done_label}"
            ),
        );
    } else {
        asm::label(asm, &loop_label);
        asm::instruction(
            asm,
            format_args!(
                "and x20, x16, #{}\n  cmp x20, #10\n  add x20, x20, #48\n  add x20, x20, #39, ge\n  strb w20, [x17, #-1]!\n  lsr x16, x16, #{}\n  cbnz x16, {loop_label}\n  b {done_label}",
                base - 1,
                if base == 16 { 4 } else { 1 }
            ),
        );
    }
    if signed {
        asm::label(asm, &sign_label);
        asm::instruction(
            asm,
            format_args!(
                "neg x16, x16\n  bl {loop_label}\n  mov w20, #45\n  strb w20, [x17, #-1]!\n  b {done_label}"
            ),
        );
    }
    asm::label(asm, &zero_label);
    asm::instruction(asm, "mov w20, #48\n  strb w20, [x17, #-1]!");
    asm::label(asm, &done_label);
    for byte in prefix.as_bytes().iter().rev() {
        asm::instruction(
            asm,
            format_args!("mov w20, #{byte}\n  strb w20, [x17, #-1]!"),
        );
    }
    asm::instruction(
        asm,
        format_args!(
            "mov x0, #1\n  mov x1, x17\n  adrp x21, {buffer}\n  add x21, x21, :lo12:{buffer}\n  add x21, x21, #128\n  sub x2, x21, x1\n  mov x8, #64"
        ),
    );
    asm::svc(asm);
    Ok(())
}

fn normalize_print_width(asm: &mut String, format: ir::PrintFormat) -> Result<(), String> {
    let opcode = match format {
        ir::PrintFormat::SignedDecimal(crate::ast::MemoryWidth::I8) => Some("sxtb"),
        ir::PrintFormat::SignedDecimal(crate::ast::MemoryWidth::I16) => Some("sxth"),
        ir::PrintFormat::SignedDecimal(crate::ast::MemoryWidth::I32) => Some("sxtw"),
        ir::PrintFormat::UnsignedDecimal(crate::ast::MemoryWidth::U8) => Some("uxtb"),
        ir::PrintFormat::UnsignedDecimal(crate::ast::MemoryWidth::U16) => Some("uxth"),
        ir::PrintFormat::UnsignedDecimal(crate::ast::MemoryWidth::U32) => Some("uxtw"),
        _ => None,
    };
    if let Some(opcode) = opcode {
        asm::instruction(asm, format_args!("{opcode} x16, w16"));
    }
    Ok(())
}

fn infer_print_format(
    source: &ir::Operand,
    slot_kinds: &HashMap<String, StackSlotKind>,
) -> Result<ir::PrintFormat, String> {
    match source {
        ir::Operand::Immediate(_) => {
            Ok(ir::PrintFormat::SignedDecimal(crate::ast::MemoryWidth::I64))
        }
        ir::Operand::Memory { width, .. } => match width {
            Some(width) if is_signed_integer_width(*width) => {
                Ok(ir::PrintFormat::SignedDecimal(*width))
            }
            Some(crate::ast::MemoryWidth::U8)
            | Some(crate::ast::MemoryWidth::U16)
            | Some(crate::ast::MemoryWidth::U32)
            | Some(crate::ast::MemoryWidth::U64) => {
                Ok(ir::PrintFormat::UnsignedDecimal(width.unwrap()))
            }
            Some(crate::ast::MemoryWidth::Ptr) => Ok(ir::PrintFormat::Pointer),
            _ => unsupported("inferred runtime printing"),
        },
        ir::Operand::Name(name) => match slot_kinds.get(name) {
            Some(StackSlotKind::Scalar(width)) => Ok(print_format_for_width(*width)),
            _ => unsupported("inferred runtime printing for string or unknown binding"),
        },
        ir::Operand::StringProperty { property, .. } => Ok(match property {
            ir::StringProperty::Len => {
                ir::PrintFormat::UnsignedDecimal(crate::ast::MemoryWidth::U64)
            }
            ir::StringProperty::Ptr => ir::PrintFormat::Pointer,
        }),
        _ => unsupported("inferred runtime printing for register or binding"),
    }
}

fn print_format_for_width(width: crate::ast::MemoryWidth) -> ir::PrintFormat {
    match width {
        crate::ast::MemoryWidth::I8
        | crate::ast::MemoryWidth::I16
        | crate::ast::MemoryWidth::I32
        | crate::ast::MemoryWidth::I64 => ir::PrintFormat::SignedDecimal(width),
        crate::ast::MemoryWidth::U8
        | crate::ast::MemoryWidth::U16
        | crate::ast::MemoryWidth::U32
        | crate::ast::MemoryWidth::U64 => ir::PrintFormat::UnsignedDecimal(width),
        crate::ast::MemoryWidth::Ptr => ir::PrintFormat::Pointer,
        _ => ir::PrintFormat::Infer,
    }
}

fn emit_linux_reserve(
    asm: &mut String,
    dst: &ir::Operand,
    len: &ir::Operand,
    slots: &HashMap<String, usize>,
    slot_kinds: &HashMap<String, StackSlotKind>,
) -> Result<(), String> {
    validate_linux_size_operand(len, slots, slot_kinds, "reserve size")?;
    let ir::Operand::TargetRegister(register) = dst else {
        return unsupported("memory reserve destination");
    };
    if !is_aarch64_x_register(register) {
        return Err(String::from(
            "AArch64 reserve destination must be a 64-bit x register",
        ));
    }
    emit_value(asm, "x0", len, slots)?;
    asm::instruction(
        asm,
        "mov x1, #0\n  mov x2, #3\n  mov x3, #34\n  mov x4, #-1\n  mov x5, #0\n  mov x8, #222",
    );
    asm::svc(asm);
    if register != "x0" {
        asm::mov(asm, register, "x0");
    }
    Ok(())
}

fn validate_linux_size_operand(
    operand: &ir::Operand,
    slots: &HashMap<String, usize>,
    slot_kinds: &HashMap<String, StackSlotKind>,
    description: &str,
) -> Result<(), String> {
    let valid = match operand {
        ir::Operand::Immediate(value) => *value >= 0,
        ir::Operand::TargetRegister(register) => is_aarch64_x_register(register),
        ir::Operand::Memory { width, .. } => width.is_some_and(|width| width.size() == 8),
        ir::Operand::Name(name) => {
            matches!(
                slot_kinds.get(name),
                Some(StackSlotKind::Scalar(
                    crate::ast::MemoryWidth::I64
                        | crate::ast::MemoryWidth::U64
                        | crate::ast::MemoryWidth::Ptr,
                ))
            ) && slots.contains_key(name)
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "AArch64 {description} must be a non-negative immediate or 64-bit integer operand"
        ))
    }
}

fn validate_linux_pointer_operand(
    operand: &ir::Operand,
    slots: &HashMap<String, usize>,
    slot_kinds: &HashMap<String, StackSlotKind>,
    description: &str,
) -> Result<(), String> {
    let valid = match operand {
        ir::Operand::Pointer(_) | ir::Operand::AddressOf(_) => true,
        ir::Operand::TargetRegister(register) => is_aarch64_x_register(register),
        ir::Operand::Memory { width, .. } => width.is_some_and(|width| width.size() == 8),
        ir::Operand::Name(name) => {
            matches!(
                slot_kinds.get(name),
                Some(StackSlotKind::Scalar(
                    crate::ast::MemoryWidth::I64
                        | crate::ast::MemoryWidth::U64
                        | crate::ast::MemoryWidth::Ptr,
                ))
            ) && slots.contains_key(name)
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "AArch64 {description} must be an address or 64-bit integer operand"
        ))
    }
}

fn validate_read_destination(
    operand: &ir::Operand,
    slots: &HashMap<String, usize>,
    slot_kinds: &HashMap<String, StackSlotKind>,
) -> Result<(), String> {
    let valid = matches!(operand, ir::Operand::Pointer(_) | ir::Operand::AddressOf(_))
        || matches!(operand, ir::Operand::TargetRegister(register) if is_aarch64_x_register(register))
        || matches!(operand, ir::Operand::Name(name) if matches!(
            slot_kinds.get(name),
            Some(StackSlotKind::Scalar(
                crate::ast::MemoryWidth::I64
                    | crate::ast::MemoryWidth::U64
                    | crate::ast::MemoryWidth::Ptr,
            ))
        ) && slots.contains_key(name));
    if valid {
        Ok(())
    } else {
        Err(String::from(
            "AArch64 read destination must be an address or 64-bit x register",
        ))
    }
}

fn emit_control_target(
    asm: &mut String,
    target: &ir::ControlTarget,
    slots: &HashMap<String, usize>,
    slot_kinds: &HashMap<String, StackSlotKind>,
) -> Result<(), String> {
    match target {
        ir::ControlTarget::Label(target) => asm::branch(asm, "b", target),
        ir::ControlTarget::Operand(target) => {
            validate_control_target(target, slot_kinds, "jump")?;
            emit_value(asm, "x16", target, slots)?;
            asm::branch(asm, "br", "x16");
        }
    }
    Ok(())
}

fn validate_control_target(
    operand: &ir::Operand,
    slot_kinds: &HashMap<String, StackSlotKind>,
    instruction: &str,
) -> Result<(), String> {
    let valid = match operand {
        ir::Operand::TargetRegister(register) => is_aarch64_x_register(register),
        ir::Operand::Memory { width, .. } => width.is_some_and(|width| {
            matches!(
                width,
                crate::ast::MemoryWidth::I64
                    | crate::ast::MemoryWidth::U64
                    | crate::ast::MemoryWidth::Ptr
            )
        }),
        ir::Operand::Name(name) => matches!(
            slot_kinds.get(name),
            Some(StackSlotKind::Scalar(
                crate::ast::MemoryWidth::I64
                    | crate::ast::MemoryWidth::U64
                    | crate::ast::MemoryWidth::Ptr,
            ))
        ),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "AArch64 indirect {instruction} target must be a 64-bit integer operand"
        ))
    }
}

fn emit_value(
    asm: &mut String,
    destination: &str,
    source: &ir::Operand,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    match source {
        ir::Operand::Immediate(value) => {
            asm::mov(asm, destination, format_args!("#{value}"));
        }
        ir::Operand::TargetRegister(register) => {
            if register != destination {
                asm::mov(asm, destination, register);
            }
        }
        ir::Operand::Memory { address, width } => {
            if matches!(
                width,
                Some(crate::ast::MemoryWidth::F32 | crate::ast::MemoryWidth::F64)
            ) {
                emit_float_memory_load(asm, destination, address, width.unwrap())?;
            } else if width.is_some() {
                let address = memory_address_or_materialize(asm, address)?;
                asm::load(
                    asm,
                    integer_load_opcode(*width)?,
                    memory_register(destination, *width),
                    address,
                );
            } else {
                let address = memory_address_or_materialize(asm, address)?;
                asm::load(asm, "ldr", destination, address);
            }
        }
        ir::Operand::Name(name) => {
            let slot = stack_operand(name, None, slots)?;
            emit_value(asm, destination, &slot, slots)?;
        }
        ir::Operand::StringProperty { name, property } => {
            let offset = *slots
                .get(name)
                .ok_or_else(|| format!("Unknown string stack slot {name:?}"))?
                + if matches!(property, ir::StringProperty::Len) {
                    8
                } else {
                    0
                };
            asm::load(asm, "ldr", destination, format_args!("[x29, #{offset}]"));
        }
        ir::Operand::Pointer(name) => {
            asm::instruction(
                asm,
                format_args!(
                    "adrp {destination}, {name}\n  add {destination}, {destination}, :lo12:{name}"
                ),
            );
        }
        ir::Operand::AddressOf(address) => emit_address(asm, destination, address)?,
        _ => return unsupported("runtime operand"),
    }
    Ok(())
}

fn emit_address_or_value(
    asm: &mut String,
    destination: &str,
    source: &ir::Operand,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    match source {
        ir::Operand::Memory { address, .. } => emit_address(asm, destination, address),
        ir::Operand::Name(name) => {
            let slot = stack_operand(name, None, slots)?;
            let ir::Operand::Memory { address, .. } = slot else {
                unreachable!()
            };
            emit_address(asm, destination, &address)
        }
        _ => emit_value(asm, destination, source, slots),
    }
}

fn emit_address(asm: &mut String, destination: &str, address: &ir::Address) -> Result<(), String> {
    validate_address_registers(address)?;
    match &address.first {
        ir::AddressTerm::TargetRegister(register) => {
            if register != destination {
                asm::mov(asm, destination, register);
            }
        }
        ir::AddressTerm::Name(name) => {
            asm::instruction(
                asm,
                format_args!(
                    "adrp {destination}, {name}\n  add {destination}, {destination}, :lo12:{name}"
                ),
            );
        }
        ir::AddressTerm::Immediate(value) => {
            asm::mov(asm, destination, format_args!("#{value}"));
        }
        ir::AddressTerm::ScaledTargetRegister { register, scale } => {
            let shift = address_scale_shift(*scale)?;
            asm::mov(asm, destination, "#0");
            asm::instruction(
                asm,
                format_args!("add {destination}, {destination}, {register}, lsl #{shift}"),
            );
        }
    }
    for (operator, term) in &address.rest {
        match term {
            ir::AddressTerm::Immediate(value) => {
                let opcode = match operator {
                    ir::AddressOperator::Add => "add",
                    ir::AddressOperator::Subtract => "sub",
                };
                asm::instruction(
                    asm,
                    format_args!("{opcode} {destination}, {destination}, #{value}"),
                );
            }
            ir::AddressTerm::TargetRegister(register) => {
                let opcode = match operator {
                    ir::AddressOperator::Add => "add",
                    ir::AddressOperator::Subtract => "sub",
                };
                asm::instruction(
                    asm,
                    format_args!("{opcode} {destination}, {destination}, {register}"),
                );
            }
            ir::AddressTerm::ScaledTargetRegister { register, scale } => {
                let shift = address_scale_shift(*scale)?;
                let opcode = match operator {
                    ir::AddressOperator::Add => "add",
                    ir::AddressOperator::Subtract => "sub",
                };
                asm::instruction(
                    asm,
                    format_args!("{opcode} {destination}, {destination}, {register}, lsl #{shift}"),
                );
            }
            ir::AddressTerm::Name(name) => {
                if address_uses_register(address, "x14") {
                    return unsupported("address uses the symbol scratch register");
                }
                asm::instruction(
                    asm,
                    format_args!(
                        "adrp x14, {name}\n  add x14, x14, :lo12:{name}\n  add {destination}, {destination}, x14"
                    ),
                );
            }
        }
    }
    Ok(())
}

fn emit_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    value: &ir::Value,
    slots: &HashMap<String, usize>,
    slot_kinds: &HashMap<String, StackSlotKind>,
    constants: &HashMap<String, ir::ConstValue>,
    target: crate::backend::Target,
) -> Result<(), String> {
    if let ir::Operand::Memory { address, width } = dst {
        if let ir::Value::FloatBinary {
            width: value_width,
            op,
            lhs,
            rhs,
        } = value
        {
            if width != &Some(*value_width) {
                return unsupported("floating-point memory width mismatch");
            }
            let suffix = match value_width {
                crate::ast::MemoryWidth::F32 => "s",
                crate::ast::MemoryWidth::F64 => "d",
                _ => return unsupported("floating-point memory width"),
            };
            emit_float_binary(asm, "v16", *value_width, *op, lhs, rhs, slots)?;
            asm::store(
                asm,
                "str",
                format_args!("{suffix}16"),
                memory_address(address)?,
            );
            return Ok(());
        }
        if let ir::Value::IntrinsicCall {
            op,
            width: value_width,
            args,
        } = value
        {
            if width != &Some(*value_width) {
                return unsupported("intrinsic memory width mismatch");
            }
            let is_float = matches!(
                value_width,
                crate::ast::MemoryWidth::F32 | crate::ast::MemoryWidth::F64
            );
            let temp = if is_float { "v16" } else { "x16" };
            emit_intrinsic(asm, temp, *op, *value_width, args, slots)?;
            let register = if is_float {
                if matches!(value_width, crate::ast::MemoryWidth::F32) {
                    "s16".to_owned()
                } else {
                    "d16".to_owned()
                }
            } else {
                integer_register_for_width("x16", *value_width)
            };
            let opcode = if is_float {
                "str"
            } else {
                integer_store_opcode(Some(*value_width))?
            };
            let address = memory_address_or_materialize(asm, address)?;
            asm::store(asm, opcode, register, address);
            return Ok(());
        }
        if let ir::Value::StringBytes { value } = value {
            if value.is_empty() {
                return Ok(());
            }
            emit_address(asm, "x16", address)?;
            for (index, byte) in value.as_bytes().iter().enumerate() {
                asm::mov(asm, "w17", format_args!("#{byte}"));
                if index == 0 {
                    asm::store(asm, "strb", "w17", "[x16]");
                } else {
                    asm::store(asm, "strb", "w17", format_args!("[x16, #{index}]"));
                }
            }
            return Ok(());
        }
        let ir::Value::Operand(src) = value else {
            return unsupported("memory assignment value");
        };
        if !width.is_some_and(crate::ast::MemoryWidth::is_float) && is_float_operand(src, slots) {
            return Err(String::from(
                "AArch64 integer memory destination cannot use a floating-point source",
            ));
        }
        if matches!(
            width,
            Some(crate::ast::MemoryWidth::F32 | crate::ast::MemoryWidth::F64)
        ) {
            let suffix = if matches!(width, Some(crate::ast::MemoryWidth::F32)) {
                "s"
            } else {
                "d"
            };
            emit_float_operand(asm, "v16", suffix, src, slots)?;
            let address = memory_address_or_materialize(asm, address)?;
            asm::store(asm, "str", format_args!("{suffix}16"), address);
        } else {
            let destination_width = width
                .ok_or_else(|| String::from("AArch64 integer memory destination needs a width"))?;
            validate_integer_memory_move_source(src, destination_width, slots, slot_kinds)?;
            let source = if let ir::Operand::TargetRegister(register) = src {
                narrow_register(register, *width)
            } else {
                emit_value(asm, "x16", src, slots)?;
                narrow_register("x16", *width)
            };
            let address = memory_address_or_materialize(asm, address)?;
            asm::store(asm, integer_store_opcode(*width)?, source, address);
        }
        return Ok(());
    }

    let ir::Operand::TargetRegister(dst) = dst else {
        return unsupported("assignment destination");
    };

    match value {
        ir::Value::Operand(ir::Operand::Name(name)) => match constants.get(name) {
            Some(ir::ConstValue::Integer { value, .. }) => {
                if dst.starts_with('v') || dst.starts_with('s') || dst.starts_with('d') {
                    return Err(String::from(
                        "AArch64 integer constant cannot be assigned to a floating-point register",
                    ));
                }
                asm::mov(asm, dst, format_args!("#{value}"));
            }
            Some(ir::ConstValue::Float { value, width }) => {
                let suffix = match width {
                    crate::ast::MemoryWidth::F32 => "s",
                    crate::ast::MemoryWidth::F64 => "d",
                    _ => return unsupported("constant floating-point width"),
                };
                emit_float_operand(
                    asm,
                    dst,
                    suffix,
                    &ir::Operand::FloatLiteral(value.clone()),
                    slots,
                )?;
            }
            Some(ir::ConstValue::String(_)) => {
                return unsupported("string constant assignment");
            }
            None => return Err(format!("Unknown constant binding {name:?}")),
        },
        ir::Value::Operand(ir::Operand::Immediate(value)) => {
            asm::mov(asm, dst, format_args!("#{value}"));
        }
        ir::Value::Operand(ir::Operand::TargetRegister(src)) => {
            validate_register_move(dst, src)?;
            asm::mov(asm, dst, src);
        }
        ir::Value::Operand(ir::Operand::Memory { address, width }) => {
            if matches!(
                width,
                Some(crate::ast::MemoryWidth::F32 | crate::ast::MemoryWidth::F64)
            ) {
                emit_float_memory_load(asm, dst, address, width.unwrap())?;
            } else {
                let address = memory_address_or_materialize(asm, address)?;
                asm::load(
                    asm,
                    integer_load_opcode(*width)?,
                    memory_register(dst, *width),
                    address,
                );
            }
        }
        ir::Value::Binary { op, lhs, rhs } => {
            validate_integer_binary_scratch_conflicts(lhs, rhs)?;
            let lhs = integer_source(asm, lhs, "x16", slots)?;
            let rhs = integer_source(asm, rhs, "x17", slots)?;
            let opcode = integer_opcode(*op)?;
            asm::instruction(asm, format_args!("{opcode} {dst}, {lhs}, {rhs}"));
        }
        ir::Value::BitwiseUnary {
            op,
            operand: source,
        } => {
            let operand = integer_source(asm, source, "x16", slots)?;
            let opcode = bitwise_unary_opcode(*op);
            asm::instruction(asm, format_args!("{opcode} {dst}, {operand}"));
        }
        ir::Value::Expression { op, lhs, rhs } => {
            emit_expression(asm, dst, op, lhs, rhs, slots)?;
        }
        ir::Value::FloatBinary {
            width,
            op,
            lhs,
            rhs,
        } => emit_float_binary(asm, dst, *width, *op, lhs, rhs, slots)?,
        ir::Value::IntrinsicCall { op, width, args } => {
            emit_intrinsic(asm, dst, *op, *width, args, slots)?;
        }
        ir::Value::Operand(ir::Operand::Cast { operand, width }) => {
            emit_cast(asm, dst, operand, *width, slots, slot_kinds)?;
        }
        ir::Value::Operand(ir::Operand::Converted {
            operand,
            conversion,
        }) => {
            if !is_aarch64_x_register(dst) {
                return Err(String::from(
                    "AArch64 width conversion destination must be a 64-bit x register",
                ));
            }
            let source_width = aarch64_integer_operand_width(operand, slot_kinds, constants)?;
            let (opcode, source_register) = match (conversion, source_width) {
                (ir::WidthConversion::SignExtend, crate::ast::MemoryWidth::I8) => ("sxtb", "w16"),
                (ir::WidthConversion::SignExtend, crate::ast::MemoryWidth::I16) => ("sxth", "w16"),
                (ir::WidthConversion::SignExtend, crate::ast::MemoryWidth::I32) => ("sxtw", "w16"),
                (ir::WidthConversion::ZeroExtend, crate::ast::MemoryWidth::U8) => ("uxtb", "w16"),
                (ir::WidthConversion::ZeroExtend, crate::ast::MemoryWidth::U16) => ("uxth", "w16"),
                (ir::WidthConversion::ZeroExtend, crate::ast::MemoryWidth::U32) => ("uxtw", "w16"),
                (_, width) => {
                    return Err(format!(
                        "AArch64 width conversion source must be a narrower matching integer operand, found {}",
                        width.name()
                    ));
                }
            };
            if let ir::Operand::TargetRegister(register) = &**operand {
                asm::mov(asm, source_register, register);
            } else if let ir::Operand::Name(name) = &**operand {
                if let Some(ir::ConstValue::Integer { value, .. }) = constants.get(name) {
                    asm::mov(asm, "x16", format_args!("#{value}"));
                } else {
                    emit_value(asm, "x16", operand, slots)?;
                }
            } else {
                emit_value(asm, "x16", operand, slots)?;
            }
            asm::instruction(asm, format_args!("{opcode} {dst}, {source_register}"));
        }
        ir::Value::Condition(condition) => {
            let true_label = format!(".L.__subsea.aarch64.condition_true_{}", asm.len());
            let done_label = format!(".L.__subsea.aarch64.condition_done_{}", asm.len());
            emit_condition_branch(asm, condition, &true_label, true, slots)?;
            asm::mov(asm, dst, "#0");
            asm::branch(asm, "b", &done_label);
            asm::label(asm, &true_label);
            asm::mov(asm, dst, "#1");
            asm::label(asm, &done_label);
        }
        ir::Value::PlatformReserve { len } => {
            if !target.supports_runtime(crate::backend::RuntimeOperation::Reserve) {
                return unsupported("linux.reserve on freestanding target");
            }
            AArch64RuntimeEmitter {
                slots,
                slot_kinds,
                constants,
            }
            .emit_reserve(asm, &ir::Operand::TargetRegister(dst.clone()), len)
            .map_err(|error| error.message)?
        }
        ir::Value::Operand(source) => emit_value(asm, dst, source, slots)?,
        _ => return unsupported("assignment value"),
    }
    Ok(())
}

fn emit_expression(
    asm: &mut String,
    destination: &str,
    op: &ExprOp,
    lhs: &ir::Value,
    rhs: &ir::Value,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    if matches!(op, ExprOp::Power) {
        validate_power_expression_operand(lhs, "base")?;
        validate_power_expression_operand(rhs, "exponent")?;
        if let ir::Value::Operand(ir::Operand::Immediate(value)) = rhs {
            if *value < 0 {
                return Err(String::from("Power exponent must be non-negative"));
            }
        }
    }
    emit_value_into_register(asm, "x16", lhs, slots)?;
    if matches!(rhs, ir::Value::Expression { .. }) {
        asm::instruction(asm, "str x16, [sp, #-16]!");
        emit_value_into_register(asm, "x17", rhs, slots)?;
        asm::instruction(asm, "ldr x16, [sp], #16");
    } else {
        emit_value_into_register(asm, "x17", rhs, slots)?;
    }
    match op {
        ExprOp::Math(op) => {
            let opcode = integer_opcode(*op)?;
            asm::instruction(asm, format_args!("{opcode} x16, x16, x17"));
        }
        ExprOp::Divide { signed } => {
            emit_division_validation(asm, *signed)?;
            asm::instruction(
                asm,
                format_args!("{} x16, x16, x17", if *signed { "sdiv" } else { "udiv" }),
            );
        }
        ExprOp::Modulo { signed } => {
            emit_division_validation(asm, *signed)?;
            asm::instruction(
                asm,
                format_args!(
                    "{} x18, x16, x17\n  msub x16, x18, x17, x16",
                    if *signed { "sdiv" } else { "udiv" }
                ),
            );
        }
        ExprOp::Power => {
            let loop_label = format!(".L.__subsea.aarch64.power_loop_{}", asm.len());
            let done_label = format!(".L.__subsea.aarch64.power_done_{}", asm.len());
            asm::mov(asm, "x18", "#1");
            asm::label(asm, &loop_label);
            asm::instruction(
                asm,
                format_args!(
                    "cbz x17, {done_label}\n  mul x18, x18, x16\n  sub x17, x17, #1\n  b {loop_label}"
                ),
            );
            asm::label(asm, &done_label);
            asm::mov(asm, "x16", "x18");
        }
    }
    if destination != "x16" {
        asm::mov(asm, destination, "x16");
    }
    Ok(())
}

fn emit_division_validation(asm: &mut String, signed: bool) -> Result<(), String> {
    let invalid = format!(".L.__subsea.aarch64.invalid_division_{}", asm.len());
    let done = format!(".L.__subsea.aarch64.valid_division_{}", asm.len());
    asm::instruction(asm, "cbz x17, ".to_owned() + &invalid);
    if signed {
        asm::mov(asm, "x18", "#1");
        asm::instruction(asm, "lsl x18, x18, #63");
        asm::instruction(asm, "cmp x16, x18");
        asm::branch(asm, "b.ne", &done);
        asm::instruction(asm, "cmn x17, #1");
        asm::branch(asm, "b.eq", &invalid);
    }
    asm::branch(asm, "b", &done);
    asm::label(asm, &invalid);
    asm::instruction(asm, "brk #0");
    asm::label(asm, &done);
    Ok(())
}

fn validate_power_expression_operand(value: &ir::Value, role: &str) -> Result<(), String> {
    let ir::Value::Operand(operand) = value else {
        return Ok(());
    };
    match operand {
        ir::Operand::FloatLiteral(_) => Err(format!("Power {role} must be an integer operand")),
        ir::Operand::Pointer(_) | ir::Operand::AddressOf(_) => {
            Err(format!("Power {role} cannot be an address-of operand"))
        }
        _ => Ok(()),
    }
}

fn emit_value_into_register(
    asm: &mut String,
    destination: &str,
    value: &ir::Value,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    match value {
        ir::Value::Operand(source) => emit_value(asm, destination, source, slots),
        ir::Value::Expression { op, lhs, rhs } => {
            emit_expression(asm, destination, op, lhs, rhs, slots)
        }
        _ => unsupported("arithmetic expression value"),
    }
}

fn integer_source(
    asm: &mut String,
    source: &ir::Operand,
    scratch: &str,
    slots: &HashMap<String, usize>,
) -> Result<String, String> {
    match source {
        ir::Operand::Immediate(_) | ir::Operand::TargetRegister(_) => operand(source, slots),
        _ => {
            emit_value(asm, scratch, source, slots)?;
            Ok(scratch.to_owned())
        }
    }
}

fn validate_integer_binary_scratch_conflicts(
    lhs: &ir::Operand,
    rhs: &ir::Operand,
) -> Result<(), String> {
    let lhs_needs_load = !matches!(
        lhs,
        ir::Operand::Immediate(_) | ir::Operand::TargetRegister(_)
    );
    let rhs_needs_load = !matches!(
        rhs,
        ir::Operand::Immediate(_) | ir::Operand::TargetRegister(_)
    );
    if lhs_needs_load && matches!(rhs, ir::Operand::TargetRegister(register) if register == "x16") {
        return Err(String::from(
            "AArch64 integer arithmetic cannot use x16 as the right register when the left operand needs scratch x16",
        ));
    }
    if rhs_needs_load && matches!(lhs, ir::Operand::TargetRegister(register) if register == "x17") {
        return Err(String::from(
            "AArch64 integer arithmetic cannot use x17 as the left register when the right operand needs scratch x17",
        ));
    }
    Ok(())
}

fn emit_float_binary(
    asm: &mut String,
    destination: &str,
    width: crate::ast::MemoryWidth,
    op: crate::ast::FloatMathOp,
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    let suffix = match width {
        crate::ast::MemoryWidth::F32 => "s",
        crate::ast::MemoryWidth::F64 => "d",
        _ => return unsupported("floating-point width"),
    };
    let destination = float_register(destination, suffix)?;
    validate_float_binary_scratch_conflicts(lhs, rhs, suffix)?;
    emit_float_operand(asm, "v16", suffix, lhs, slots)?;
    emit_float_operand(asm, "v17", suffix, rhs, slots)?;
    let opcode = match op {
        crate::ast::FloatMathOp::Add => "fadd",
        crate::ast::FloatMathOp::Divide => "fdiv",
        crate::ast::FloatMathOp::Multiply => "fmul",
        crate::ast::FloatMathOp::Subtract => "fsub",
    };
    asm::instruction(
        asm,
        format_args!("{opcode} {destination}, {suffix}16, {suffix}17"),
    );
    Ok(())
}

fn validate_float_binary_scratch_conflicts(
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    suffix: &str,
) -> Result<(), String> {
    let lhs_needs_load = !matches!(lhs, ir::Operand::TargetRegister(_));
    let rhs_needs_load = !matches!(rhs, ir::Operand::TargetRegister(_));
    let is_scratch = |operand: &ir::Operand, register: &str| {
        matches!(operand, ir::Operand::TargetRegister(name)
            if float_register(name, suffix).ok().as_deref() == Some(register))
    };
    if lhs_needs_load && is_scratch(rhs, &format!("{suffix}16")) {
        return Err(String::from(
            "AArch64 floating-point arithmetic right operand conflicts with v16 scratch register",
        ));
    }
    if rhs_needs_load && is_scratch(lhs, &format!("{suffix}17")) {
        return Err(String::from(
            "AArch64 floating-point arithmetic left operand conflicts with v17 scratch register",
        ));
    }
    Ok(())
}

fn float_register(register: &str, suffix: &str) -> Result<String, String> {
    if let Some(index) = register.strip_prefix('v') {
        if index.parse::<u8>().is_ok_and(|index| index <= 31) {
            return Ok(format!("{suffix}{index}"));
        }
    }
    if (register.starts_with('s') && suffix == "s") || (register.starts_with('d') && suffix == "d")
    {
        return Ok(register.to_owned());
    }
    unsupported("floating-point destination register")
}

fn emit_float_memory_load(
    asm: &mut String,
    destination: &str,
    address: &ir::Address,
    width: crate::ast::MemoryWidth,
) -> Result<(), String> {
    let suffix = match width {
        crate::ast::MemoryWidth::F32 => "s",
        crate::ast::MemoryWidth::F64 => "d",
        _ => return unsupported("floating-point memory width"),
    };
    let destination = float_register(destination, suffix)?;
    let address = memory_address_or_materialize(asm, address)?;
    asm::load(asm, "ldr", destination, address);
    Ok(())
}

fn emit_float_operand(
    asm: &mut String,
    register: &str,
    suffix: &str,
    source: &ir::Operand,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    validate_float_operand_width(source, suffix, slots)?;
    let register = format!("{suffix}{}", register.trim_start_matches('v'));
    match source {
        ir::Operand::TargetRegister(source) => {
            let source = float_register(source, suffix)?;
            asm::instruction(asm, format_args!("fmov {register}, {source}"));
        }
        ir::Operand::Memory { address, .. } => {
            let address = memory_address_or_materialize(asm, address)?;
            asm::load(asm, "ldr", register, address);
        }
        ir::Operand::FloatLiteral(value) => {
            let label = format!(".L.__subsea.aarch64.float_{}", asm.len());
            let directive = if suffix == "s" { ".float" } else { ".double" };
            asm::section(asm, "rodata");
            asm::label(asm, &label);
            asm::directive(asm, format_args!("{directive} {value}"));
            asm::text(asm);
            asm::instruction(
                asm,
                format_args!(
                    "adrp x16, {label}\n  add x16, x16, :lo12:{label}\n  ldr {register}, [x16]"
                ),
            );
        }
        ir::Operand::Name(name) => {
            let slot = stack_operand(name, None, slots)?;
            emit_float_operand(asm, &register[..3], suffix, &slot, slots)?;
        }
        _ => return unsupported("floating-point operand"),
    }
    Ok(())
}

fn validate_float_operand_width(
    source: &ir::Operand,
    suffix: &str,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    let expected = if suffix == "s" {
        crate::ast::MemoryWidth::F32
    } else {
        crate::ast::MemoryWidth::F64
    };
    let declared = match source {
        ir::Operand::Memory { width, .. } => *width,
        ir::Operand::Name(name) => match stack_operand(name, None, slots)? {
            ir::Operand::Memory { width, .. } => width,
            _ => None,
        },
        _ => None,
    };
    if let Some(width) = declared {
        if width != expected {
            return Err(format!(
                "AArch64 floating-point operand width must be {}, found {}",
                expected.name(),
                width.name()
            ));
        }
    }
    Ok(())
}

fn validate_register_move(destination: &str, source: &str) -> Result<(), String> {
    let destination_float = destination.starts_with('v')
        || destination.starts_with('s')
        || destination.starts_with('d');
    let source_float =
        source.starts_with('v') || source.starts_with('s') || source.starts_with('d');
    if destination_float != source_float {
        return Err(String::from(
            "AArch64 register move cannot mix integer and floating-point registers",
        ));
    }
    if !destination_float && destination.starts_with('x') != source.starts_with('x') {
        return Err(String::from(
            "AArch64 register move cannot mix 32-bit and 64-bit integer registers",
        ));
    }
    Ok(())
}

fn validate_integer_memory_move_source(
    source: &ir::Operand,
    destination_width: crate::ast::MemoryWidth,
    slots: &HashMap<String, usize>,
    slot_kinds: &HashMap<String, StackSlotKind>,
) -> Result<(), String> {
    if let ir::Operand::TargetRegister(register) = source {
        if destination_width.size() == 8 && !is_aarch64_x_register(register) {
            return Err(String::from(
                "AArch64 64-bit memory moves require an x-register source",
            ));
        }
        return Ok(());
    }
    let source_width = match source {
        ir::Operand::Memory { width, .. } => *width,
        ir::Operand::Name(name) => match slot_kinds.get(name) {
            Some(StackSlotKind::Scalar(width)) => Some(*width),
            _ => None,
        },
        _ => None,
    };
    if let Some(source_width) = source_width {
        if source_width != destination_width {
            return Err(format!(
                "AArch64 integer memory move cannot use {}-bit source with {}-bit destination",
                source_width.size() * 8,
                destination_width.size() * 8
            ));
        }
    }
    let _ = slots;
    Ok(())
}

fn emit_intrinsic(
    asm: &mut String,
    destination: &str,
    op: crate::ast::IntrinsicOp,
    width: crate::ast::MemoryWidth,
    args: &[ir::Operand],
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    if matches!(op, crate::ast::IntrinsicOp::Sqrt)
        && !matches!(
            width,
            crate::ast::MemoryWidth::F32 | crate::ast::MemoryWidth::F64
        )
    {
        let source = args.first().ok_or("sqrt requires an operand")?;
        emit_value(asm, "x16", source, slots)?;
        let bits = integer_width_bits(width)?;
        let register = integer_register_for_width("x16", width);
        let result = integer_register_for_width(destination, width);
        let id = asm.len();
        let negative = format!(".L.__subsea.aarch64.sqrt_negative_{id}");
        if is_signed_integer_width(width) {
            asm::instruction(
                asm,
                format_args!("tbnz {register}, #{}, {negative}", bits - 1),
            );
        }
        let align = format!(".L.__subsea.aarch64.sqrt_align_{id}");
        let loop_label = format!(".L.__subsea.aarch64.sqrt_loop_{id}");
        let skip = format!(".L.__subsea.aarch64.sqrt_skip_{id}");
        let done = format!(".L.__subsea.aarch64.sqrt_done_{id}");
        asm::instruction(
            asm,
            format_args!(
                "mov {r2}, #0\n  mov {r3}, #1\n  lsl {r3}, {r3}, #{}\n{align}:\n  cmp {r3}, {r}\n  bls {loop_label}\n  lsr {r3}, {r3}, #2\n  b {align}\n{loop_label}:\n  cbz {r3}, {done}\n  add {r4}, {r2}, {r3}\n  cmp {r}, {r4}\n  blo {skip}\n  sub {r}, {r}, {r4}\n  lsr {r2}, {r2}, #1\n  add {r2}, {r2}, {r3}\n  lsr {r3}, {r3}, #2\n  b {loop_label}\n{skip}:\n  lsr {r2}, {r2}, #1\n  lsr {r3}, {r3}, #2\n  b {loop_label}\n{done}:\n  mov {result}, {r2}",
                bits - 2,
                r = register,
                r2 = integer_register_for_width("x17", width),
                r3 = integer_register_for_width("x18", width),
                r4 = integer_register_for_width("x19", width),
                result = result,
            ),
        );
        if is_signed_integer_width(width) {
            asm::label(asm, &negative);
            asm::instruction(asm, "brk #0");
        }
        return Ok(());
    }
    if matches!(
        op,
        crate::ast::IntrinsicOp::Min | crate::ast::IntrinsicOp::Max
    ) && !matches!(
        width,
        crate::ast::MemoryWidth::F32 | crate::ast::MemoryWidth::F64
    ) {
        let lhs = args.first().ok_or("min/max requires two operands")?;
        let rhs = args.get(1).ok_or("min/max requires two operands")?;
        emit_value(asm, "x16", lhs, slots)?;
        emit_value(asm, "x17", rhs, slots)?;
        let lhs_register = integer_register_for_width("x16", width);
        let rhs_register = integer_register_for_width("x17", width);
        asm::instruction(asm, format_args!("cmp {lhs_register}, {rhs_register}"));
        let condition = if matches!(op, crate::ast::IntrinsicOp::Min) {
            if is_signed_integer_width(width) {
                "lt"
            } else {
                "lo"
            }
        } else if is_signed_integer_width(width) {
            "gt"
        } else {
            "hi"
        };
        let lhs = lhs_register;
        let rhs = rhs_register;
        let destination = integer_register_for_width(destination, width);
        asm::instruction(
            asm,
            format_args!("csel {destination}, {lhs}, {rhs}, {condition}"),
        );
        return Ok(());
    }
    let suffix = match width {
        crate::ast::MemoryWidth::F32 => "s",
        crate::ast::MemoryWidth::F64 => "d",
        _ => return unsupported("non-floating intrinsic"),
    };
    let destination = float_register(destination, suffix)?;
    let first = args.first().ok_or("intrinsic requires an operand")?;
    emit_float_operand(asm, "v16", suffix, first, slots)?;
    if matches!(
        op,
        crate::ast::IntrinsicOp::Min | crate::ast::IntrinsicOp::Max
    ) {
        let second = args.get(1).ok_or("min/max requires two operands")?;
        emit_float_operand(asm, "v17", suffix, second, slots)?;
    }
    let opcode = match op {
        crate::ast::IntrinsicOp::Ceil => "frintp",
        crate::ast::IntrinsicOp::Floor => "frintm",
        crate::ast::IntrinsicOp::Max => "fmax",
        crate::ast::IntrinsicOp::Min => "fmin",
        crate::ast::IntrinsicOp::Round => "frintn",
        crate::ast::IntrinsicOp::Sqrt => "fsqrt",
        crate::ast::IntrinsicOp::Trunc => "frintz",
    };
    if matches!(
        op,
        crate::ast::IntrinsicOp::Min | crate::ast::IntrinsicOp::Max
    ) {
        asm::instruction(
            asm,
            format_args!("{opcode} {destination}, {suffix}16, {suffix}17"),
        );
    } else {
        asm::instruction(asm, format_args!("{opcode} {destination}, {suffix}16"));
    }
    Ok(())
}

fn is_signed_integer_width(width: crate::ast::MemoryWidth) -> bool {
    matches!(
        width,
        crate::ast::MemoryWidth::I8
            | crate::ast::MemoryWidth::I16
            | crate::ast::MemoryWidth::I32
            | crate::ast::MemoryWidth::I64
    )
}

fn is_unsigned_integer_width(width: crate::ast::MemoryWidth) -> bool {
    matches!(
        width,
        crate::ast::MemoryWidth::U8
            | crate::ast::MemoryWidth::U16
            | crate::ast::MemoryWidth::U32
            | crate::ast::MemoryWidth::U64
            | crate::ast::MemoryWidth::Ptr
    )
}

fn is_aarch64_x_register(register: &str) -> bool {
    register
        .strip_prefix('x')
        .is_some_and(|index| index.parse::<u8>().is_ok_and(|index| index <= 30))
}

fn validate_aarch64_pair_register(name: &str, register: &str) -> Result<(), String> {
    if is_aarch64_x_register(register) {
        Ok(())
    } else if register.strip_prefix('w').is_some() {
        Err(format!(
            "{name} must be 64-bit, found 32-bit register {register}"
        ))
    } else {
        Err(format!("{name} must be a 64-bit integer register"))
    }
}

fn aarch64_integer_operand_width(
    operand: &ir::Operand,
    slot_kinds: &HashMap<String, StackSlotKind>,
    constants: &HashMap<String, ir::ConstValue>,
) -> Result<crate::ast::MemoryWidth, String> {
    match operand {
        ir::Operand::TargetRegister(register)
            if register
                .strip_prefix('w')
                .is_some_and(|index| index.parse::<u8>().is_ok_and(|index| index <= 30)) =>
        {
            Ok(crate::ast::MemoryWidth::I32)
        }
        ir::Operand::Memory {
            width: Some(width), ..
        } if integer_width_bits(*width).is_ok() => Ok(*width),
        ir::Operand::Name(name) => match slot_kinds.get(name) {
            Some(StackSlotKind::Scalar(width)) if integer_width_bits(*width).is_ok() => Ok(*width),
            _ => match constants.get(name) {
                Some(ir::ConstValue::Integer {
                    width: Some(width), ..
                }) => Ok(*width),
                _ => Err(String::from(
                    "AArch64 width conversion source must be a known integer operand",
                )),
            },
        },
        _ => Err(String::from(
            "AArch64 width conversion source must be a known integer operand",
        )),
    }
}

fn integer_width_bits(width: crate::ast::MemoryWidth) -> Result<u32, String> {
    match width {
        crate::ast::MemoryWidth::I8 | crate::ast::MemoryWidth::U8 => Ok(8),
        crate::ast::MemoryWidth::I16 | crate::ast::MemoryWidth::U16 => Ok(16),
        crate::ast::MemoryWidth::I32 | crate::ast::MemoryWidth::U32 => Ok(32),
        crate::ast::MemoryWidth::I64
        | crate::ast::MemoryWidth::U64
        | crate::ast::MemoryWidth::Ptr => Ok(64),
        _ => unsupported("integer width"),
    }
}

fn integer_register_for_width(register: &str, width: crate::ast::MemoryWidth) -> String {
    if matches!(
        width,
        crate::ast::MemoryWidth::I8
            | crate::ast::MemoryWidth::U8
            | crate::ast::MemoryWidth::I16
            | crate::ast::MemoryWidth::U16
            | crate::ast::MemoryWidth::I32
            | crate::ast::MemoryWidth::U32
    ) {
        if let Some(index) = register.strip_prefix('x') {
            return format!("w{index}");
        }
    }
    register.to_owned()
}

fn emit_cast(
    asm: &mut String,
    destination: &str,
    source: &ir::Operand,
    width: crate::ast::MemoryWidth,
    slots: &HashMap<String, usize>,
    slot_kinds: &HashMap<String, StackSlotKind>,
) -> Result<(), String> {
    let floating_destination = matches!(
        width,
        crate::ast::MemoryWidth::F32 | crate::ast::MemoryWidth::F64
    );
    if floating_destination {
        let suffix = if matches!(width, crate::ast::MemoryWidth::F32) {
            "s"
        } else {
            "d"
        };
        let destination = float_register(destination, suffix)?;
        let source_unsigned = match source {
            ir::Operand::Memory {
                width: Some(width), ..
            } => is_unsigned_integer_width(*width),
            ir::Operand::Name(name) => match slot_kinds.get(name) {
                Some(StackSlotKind::Scalar(width)) => is_unsigned_integer_width(*width),
                _ => false,
            },
            _ => false,
        };
        let source_register = match source {
            ir::Operand::TargetRegister(register) if register.starts_with('w') => "w16",
            ir::Operand::Memory {
                width: Some(width), ..
            } if matches!(
                width,
                crate::ast::MemoryWidth::I8
                    | crate::ast::MemoryWidth::U8
                    | crate::ast::MemoryWidth::I16
                    | crate::ast::MemoryWidth::U16
                    | crate::ast::MemoryWidth::I32
                    | crate::ast::MemoryWidth::U32
            ) =>
            {
                "w16"
            }
            _ => "x16",
        };
        emit_value(asm, source_register, source, slots)?;
        let opcode = if source_unsigned { "ucvtf" } else { "scvtf" };
        asm::instruction(
            asm,
            format_args!("{opcode} {destination}, {source_register}"),
        );
    } else {
        let suffix = match source {
            ir::Operand::TargetRegister(register) if register.starts_with('s') => "s",
            ir::Operand::TargetRegister(register) if register.starts_with('d') => "d",
            ir::Operand::Memory {
                width: Some(crate::ast::MemoryWidth::F32),
                ..
            } => "s",
            ir::Operand::Memory {
                width: Some(crate::ast::MemoryWidth::F64),
                ..
            } => "d",
            ir::Operand::Name(name) => match slot_kinds.get(name) {
                Some(StackSlotKind::Scalar(crate::ast::MemoryWidth::F32)) => "s",
                Some(StackSlotKind::Scalar(crate::ast::MemoryWidth::F64)) => "d",
                _ => return unsupported("floating-point cast source width"),
            },
            _ => return unsupported("floating-point cast source width"),
        };
        emit_float_operand(asm, "v16", suffix, source, slots)?;
        emit_float_to_integer_validation(asm, suffix, width)?;
        let destination = integer_register_for_width(destination, width);
        let opcode = if is_signed_integer_width(width) {
            "fcvtzs"
        } else {
            "fcvtzu"
        };
        asm::instruction(asm, format_args!("{opcode} {destination}, {suffix}16"));
    }
    Ok(())
}

fn emit_float_to_integer_validation(
    asm: &mut String,
    suffix: &str,
    width: crate::ast::MemoryWidth,
) -> Result<(), String> {
    let bits = integer_width_bits(width)?;
    let unsigned = is_unsigned_integer_width(width);
    let invalid = format!(".L.__subsea.aarch64.invalid_cast_{}", asm.len());
    let done = format!(".L.__subsea.aarch64.valid_cast_{}", asm.len());
    if unsigned {
        asm::instruction(asm, format_args!("fcmp {suffix}16, #0.0"));
        asm::branch(asm, "b.vs", &invalid);
        asm::branch(asm, "b.lt", &invalid);
    } else {
        emit_aarch64_float_constant(asm, suffix, integer_cast_bound(bits, false, false)?);
        asm::instruction(asm, format_args!("fcmp {suffix}16, {suffix}17"));
        asm::branch(asm, "b.vs", &invalid);
        asm::branch(asm, "b.lt", &invalid);
    }
    emit_aarch64_float_constant(asm, suffix, integer_cast_bound(bits, unsigned, true)?);
    asm::instruction(asm, format_args!("fcmp {suffix}16, {suffix}17"));
    asm::branch(asm, "b.vs", &invalid);
    asm::branch(asm, "b.ge", &invalid);
    asm::branch(asm, "b", &done);
    asm::label(asm, &invalid);
    asm::instruction(asm, "brk #0");
    asm::label(asm, &done);
    Ok(())
}

fn integer_cast_bound(bits: u32, unsigned: bool, upper: bool) -> Result<&'static str, String> {
    match (bits, unsigned, upper) {
        (8, false, false) => Ok("-128.0"),
        (8, false, true) => Ok("128.0"),
        (16, false, false) => Ok("-32768.0"),
        (16, false, true) => Ok("32768.0"),
        (32, false, false) => Ok("-2147483648.0"),
        (32, false, true) => Ok("2147483648.0"),
        (64, false, false) => Ok("-9223372036854775808.0"),
        (64, false, true) => Ok("9223372036854775808.0"),
        (8, true, true) => Ok("256.0"),
        (16, true, true) => Ok("65536.0"),
        (32, true, true) => Ok("4294967296.0"),
        (64, true, true) => Ok("18446744073709551616.0"),
        _ => Err(String::from("unsupported integer cast bound")),
    }
}

fn emit_aarch64_float_constant(asm: &mut String, suffix: &str, value: &str) {
    let label = format!(".L.__subsea.aarch64.cast_constant_{}", asm.len());
    asm::section(asm, "rodata");
    asm::label(asm, &label);
    asm::directive(
        asm,
        format_args!(
            "{} {value}",
            if suffix == "s" { ".float" } else { ".double" }
        ),
    );
    asm::text(asm);
    asm::instruction(
        asm,
        format_args!("adrp x15, {label}\n  add x15, x15, :lo12:{label}\n  ldr {suffix}17, [x15]"),
    );
}

fn emit_condition_branch(
    asm: &mut String,
    condition: &ir::Condition,
    target: &str,
    branch_when_true: bool,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    if let ir::Condition::BitwiseAndZero { lhs, rhs, op } = condition {
        emit_value(asm, "x16", lhs, slots)?;
        emit_value(asm, "x17", rhs, slots)?;
        asm::instruction(asm, "and x16, x16, x17");
        let zero = matches!(op, crate::ast::CompareOp::Equal) == branch_when_true;
        asm::branch(asm, if zero { "cbz" } else { "cbnz" }, target);
        return Ok(());
    }
    let ir::Condition::Compare { lhs, op, rhs } = condition else {
        return unsupported("condition");
    };
    if is_float_operand(lhs, slots) || is_float_operand(rhs, slots) {
        let suffix = float_compare_suffix(*op)
            .or_else(|| float_operand_suffix(lhs, slots))
            .or_else(|| float_operand_suffix(rhs, slots))
            .unwrap_or("d");
        emit_float_operand(asm, "v16", suffix, lhs, slots)?;
        emit_float_operand(asm, "v17", suffix, rhs, slots)?;
        asm::instruction(asm, format_args!("fcmp {suffix}16, {suffix}17"));
        emit_float_compare_branch(asm, *op, branch_when_true, target)?;
        return Ok(());
    }
    let lhs = integer_source(asm, lhs, "x16", slots)?;
    let rhs = integer_source(asm, rhs, "x17", slots)?;
    asm::instruction(asm, format_args!("cmp {lhs}, {rhs}"));
    let opcode = compare_opcode(*op, branch_when_true)?;
    asm::branch(asm, opcode, target);
    Ok(())
}

fn is_float_operand(operand: &ir::Operand, slots: &HashMap<String, usize>) -> bool {
    matches!(operand, ir::Operand::FloatLiteral(_))
        || matches!(
            operand,
            ir::Operand::TargetRegister(name)
                if name.starts_with('v') || name.starts_with('s') || name.starts_with('d')
        )
        || matches!(
            operand,
            ir::Operand::Memory {
                width: Some(crate::ast::MemoryWidth::F32 | crate::ast::MemoryWidth::F64),
                ..
            }
        )
        || matches!(
            operand,
            ir::Operand::Name(name)
                if matches!(
                    stack_operand(name, None, slots),
                    Ok(ir::Operand::Memory {
                        width: Some(crate::ast::MemoryWidth::F32 | crate::ast::MemoryWidth::F64),
                        ..
                    })
                )
        )
}

fn float_operand_suffix(
    operand: &ir::Operand,
    slots: &HashMap<String, usize>,
) -> Option<&'static str> {
    match operand {
        ir::Operand::TargetRegister(name) if name.starts_with('s') => Some("s"),
        ir::Operand::TargetRegister(name) if name.starts_with('d') => Some("d"),
        ir::Operand::Memory {
            width: Some(crate::ast::MemoryWidth::F32),
            ..
        } => Some("s"),
        ir::Operand::Memory {
            width: Some(crate::ast::MemoryWidth::F64),
            ..
        } => Some("d"),
        ir::Operand::Name(name) => match stack_operand(name, None, slots).ok() {
            Some(ir::Operand::Memory {
                width: Some(crate::ast::MemoryWidth::F32),
                ..
            }) => Some("s"),
            Some(ir::Operand::Memory {
                width: Some(crate::ast::MemoryWidth::F64),
                ..
            }) => Some("d"),
            _ => None,
        },
        _ => None,
    }
}

fn float_compare_suffix(op: crate::ast::CompareOp) -> Option<&'static str> {
    match op {
        crate::ast::CompareOp::FloatEqual(crate::ast::MemoryWidth::F32)
        | crate::ast::CompareOp::FloatNotEqual(crate::ast::MemoryWidth::F32)
        | crate::ast::CompareOp::FloatLess(crate::ast::MemoryWidth::F32)
        | crate::ast::CompareOp::FloatLessEqual(crate::ast::MemoryWidth::F32)
        | crate::ast::CompareOp::FloatGreater(crate::ast::MemoryWidth::F32)
        | crate::ast::CompareOp::FloatGreaterEqual(crate::ast::MemoryWidth::F32) => Some("s"),
        crate::ast::CompareOp::FloatEqual(crate::ast::MemoryWidth::F64)
        | crate::ast::CompareOp::FloatNotEqual(crate::ast::MemoryWidth::F64)
        | crate::ast::CompareOp::FloatLess(crate::ast::MemoryWidth::F64)
        | crate::ast::CompareOp::FloatLessEqual(crate::ast::MemoryWidth::F64)
        | crate::ast::CompareOp::FloatGreater(crate::ast::MemoryWidth::F64)
        | crate::ast::CompareOp::FloatGreaterEqual(crate::ast::MemoryWidth::F64) => Some("d"),
        _ => None,
    }
}

fn emit_float_compare_branch(
    asm: &mut String,
    op: crate::ast::CompareOp,
    branch_when_true: bool,
    target: &str,
) -> Result<(), String> {
    let opcode = match (op, branch_when_true) {
        (crate::ast::CompareOp::FloatEqual(_), true) => "eq",
        (crate::ast::CompareOp::FloatEqual(_), false) => "ne",
        (crate::ast::CompareOp::FloatNotEqual(_), true) => "ne",
        (crate::ast::CompareOp::FloatNotEqual(_), false) => "eq",
        (crate::ast::CompareOp::FloatLess(_), true) => "lt",
        (crate::ast::CompareOp::FloatLess(_), false) => "ge",
        (crate::ast::CompareOp::FloatLessEqual(_), true) => "le",
        (crate::ast::CompareOp::FloatLessEqual(_), false) => "gt",
        (crate::ast::CompareOp::FloatGreater(_), true) => "gt",
        (crate::ast::CompareOp::FloatGreater(_), false) => "le",
        (crate::ast::CompareOp::FloatGreaterEqual(_), true) => "ge",
        (crate::ast::CompareOp::FloatGreaterEqual(_), false) => "lt",
        _ => return Err(String::from("unsupported floating-point comparison")),
    };
    asm::branch(asm, &format!("b.{opcode}"), target);
    let unordered_is_true = match op {
        crate::ast::CompareOp::FloatNotEqual(_) => branch_when_true,
        crate::ast::CompareOp::FloatEqual(_)
        | crate::ast::CompareOp::FloatLess(_)
        | crate::ast::CompareOp::FloatLessEqual(_)
        | crate::ast::CompareOp::FloatGreater(_)
        | crate::ast::CompareOp::FloatGreaterEqual(_) => !branch_when_true,
        _ => false,
    };
    if unordered_is_true {
        asm::branch(asm, "b.vs", target);
    }
    Ok(())
}

fn operand(operand: &ir::Operand, slots: &HashMap<String, usize>) -> Result<String, String> {
    match operand {
        ir::Operand::Immediate(value) => Ok(format!("#{value}")),
        ir::Operand::TargetRegister(register) => Ok(register.clone()),
        ir::Operand::Memory { address, .. } => memory_address(address),
        ir::Operand::Name(name) => {
            let ir::Operand::Memory { address, .. } = stack_operand(name, None, slots)? else {
                unreachable!()
            };
            memory_address(&address)
        }
        ir::Operand::StringProperty { name, property } => {
            let offset = *slots
                .get(name)
                .ok_or_else(|| format!("Unknown string stack slot {name:?}"))?
                + if matches!(property, ir::StringProperty::Len) {
                    8
                } else {
                    0
                };
            Ok(format!("[x29, #{offset}]"))
        }
        ir::Operand::Pointer(name) => Ok(name.clone()),
        _ => unsupported("operand"),
    }
}

fn stack_slots(layout: &ir::StackLayout, base: usize) -> HashMap<String, usize> {
    let mut offset = base;
    let mut slots = HashMap::new();
    for slot in &layout.slots {
        let name = match slot {
            ir::StackSlot::Scalar { name, .. } | ir::StackSlot::String { name } => name,
        };
        slots.insert(name.clone(), offset);
        offset += if matches!(slot, ir::StackSlot::String { .. }) {
            16
        } else {
            8
        };
    }
    slots
}

fn stack_slot_kinds(layout: &ir::StackLayout) -> HashMap<String, StackSlotKind> {
    layout
        .slots
        .iter()
        .map(|slot| match slot {
            ir::StackSlot::Scalar { name, width } => (name.clone(), StackSlotKind::Scalar(*width)),
            ir::StackSlot::String { name } => (name.clone(), StackSlotKind::String),
        })
        .collect()
}

fn stack_frame_size(layout: &ir::StackLayout) -> usize {
    let size = layout
        .slots
        .iter()
        .map(|slot| match slot {
            ir::StackSlot::Scalar { .. } => 8,
            ir::StackSlot::String { .. } => 16,
        })
        .sum::<usize>();
    size.div_ceil(16) * 16
}

fn stack_operand(
    name: &str,
    width: Option<crate::ast::MemoryWidth>,
    slots: &HashMap<String, usize>,
) -> Result<ir::Operand, String> {
    let offset = *slots
        .get(name)
        .ok_or_else(|| format!("Unknown stack slot {name:?}"))?;
    Ok(ir::Operand::Memory {
        address: ir::Address {
            first: ir::AddressTerm::TargetRegister(String::from("x29")),
            rest: if offset == 0 {
                Vec::new()
            } else {
                vec![(
                    ir::AddressOperator::Add,
                    ir::AddressTerm::Immediate(offset as i128),
                )]
            },
        },
        width,
    })
}

fn emit_stack_string(
    asm: &mut String,
    name: &str,
    value: &ir::StringInitializer,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    let offset = *slots
        .get(name)
        .ok_or_else(|| format!("Unknown string stack slot {name:?}"))?;
    match value {
        ir::StringInitializer::Literal(value) => {
            let label = format!(".L.__subsea.aarch64.stack_string_{}", asm.len());
            let bytes = value
                .as_bytes()
                .iter()
                .map(|byte| byte.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            asm::section(asm, "rodata");
            asm::label(asm, &label);
            asm::directive(asm, format_args!(".byte {bytes}"));
            asm::top_level_directive(asm, ".text");
            asm::instruction(
                asm,
                format_args!(
                    "adrp x16, {label}\n  add x16, x16, :lo12:{label}\n  str x16, [x29, #{offset}]\n  mov x16, #{}\n  str x16, [x29, #{}]",
                    value.len(),
                    offset + 8
                ),
            );
        }
        ir::StringInitializer::Slice { ptr, len } => {
            emit_address_or_value(asm, "x16", ptr, slots)?;
            asm::store(asm, "str", "x16", format_args!("[x29, #{offset}]"));
            emit_value(asm, "x16", len, slots)?;
            asm::store(asm, "str", "x16", format_args!("[x29, #{}]", offset + 8));
        }
    }
    Ok(())
}

fn memory_address(address: &ir::Address) -> Result<String, String> {
    validate_address_registers(address)?;
    let base = address_term(&address.first)?;
    let Some((operator, term)) = address.rest.first() else {
        return Ok(format!("[{base}]"));
    };
    if address.rest.len() > 1 {
        return unsupported("multiple address terms");
    }
    match term {
        ir::AddressTerm::Immediate(value) => {
            let value = match operator {
                ir::AddressOperator::Add => *value,
                ir::AddressOperator::Subtract => -*value,
            };
            Ok(format!("[{base}, #{value}]"))
        }
        ir::AddressTerm::TargetRegister(register) => {
            if *operator == ir::AddressOperator::Subtract {
                return unsupported("negative register address terms");
            }
            Ok(format!("[{base}, {register}]"))
        }
        ir::AddressTerm::ScaledTargetRegister { register, scale } => {
            if *operator == ir::AddressOperator::Subtract {
                return unsupported("negative scaled address terms");
            }
            let shift = address_scale_shift(*scale)?;
            Ok(format!("[{base}, {register}, lsl #{shift}]"))
        }
        ir::AddressTerm::Name(_) => unsupported("symbol address term"),
    }
}

fn memory_address_or_materialize(
    asm: &mut String,
    address: &ir::Address,
) -> Result<String, String> {
    if matches!(address.first, ir::AddressTerm::TargetRegister(_))
        && let Ok(address) = memory_address(address)
    {
        return Ok(address);
    }

    const SCRATCH: &str = "x15";
    if address_uses_register(address, SCRATCH) {
        return unsupported("address uses the address scratch register");
    }
    emit_address(asm, SCRATCH, address)?;
    Ok(format!("[{SCRATCH}]"))
}

fn address_uses_register(address: &ir::Address, register: &str) -> bool {
    let uses = |term: &ir::AddressTerm| {
        matches!(
            term,
            ir::AddressTerm::TargetRegister(name)
                | ir::AddressTerm::ScaledTargetRegister { register: name, .. }
                if name == register
        )
    };
    uses(&address.first) || address.rest.iter().any(|(_, term)| uses(term))
}

fn validate_address_registers(address: &ir::Address) -> Result<(), String> {
    let validate = |term: &ir::AddressTerm| match term {
        ir::AddressTerm::TargetRegister(register)
        | ir::AddressTerm::ScaledTargetRegister { register, .. }
            if is_aarch64_x_register(register) =>
        {
            Ok(())
        }
        ir::AddressTerm::TargetRegister(register)
        | ir::AddressTerm::ScaledTargetRegister { register, .. } => Err(format!(
            "AArch64 address register must be a 64-bit x register, found {register}"
        )),
        _ => Ok(()),
    };
    validate(&address.first)?;
    for (_, term) in &address.rest {
        validate(term)?;
    }
    Ok(())
}

fn operand_uses_register(operand: &ir::Operand, register: &str) -> bool {
    match operand {
        ir::Operand::TargetRegister(name) => name == register,
        ir::Operand::Memory { address, .. } | ir::Operand::AddressOf(address) => {
            address_uses_register(address, register)
        }
        ir::Operand::Converted { operand, .. } | ir::Operand::Cast { operand, .. } => {
            operand_uses_register(operand, register)
        }
        _ => false,
    }
}

fn address_scale_shift(scale: i64) -> Result<i64, String> {
    match scale {
        1 => Ok(0),
        2 => Ok(1),
        4 => Ok(2),
        8 => Ok(3),
        _ => unsupported("unsupported address scale"),
    }
}

fn address_term(term: &ir::AddressTerm) -> Result<String, String> {
    match term {
        ir::AddressTerm::TargetRegister(register) => Ok(register.clone()),
        ir::AddressTerm::Immediate(value) => Ok(format!("#{value}")),
        ir::AddressTerm::ScaledTargetRegister { register, scale } => {
            let shift = address_scale_shift(*scale)?;
            Ok(format!("{register}, lsl #{shift}"))
        }
        ir::AddressTerm::Name(name) => Ok(name.clone()),
    }
}

fn narrow_register(register: &str, width: Option<crate::ast::MemoryWidth>) -> String {
    let narrow = width.is_some_and(|width| {
        matches!(
            width,
            crate::ast::MemoryWidth::I8
                | crate::ast::MemoryWidth::I16
                | crate::ast::MemoryWidth::I32
                | crate::ast::MemoryWidth::U8
                | crate::ast::MemoryWidth::U16
                | crate::ast::MemoryWidth::U32
        )
    });
    if narrow && register.starts_with('x') {
        format!("w{}", &register[1..])
    } else {
        register.to_owned()
    }
}

fn integer_store_opcode(width: Option<crate::ast::MemoryWidth>) -> Result<&'static str, String> {
    match width {
        Some(crate::ast::MemoryWidth::I8 | crate::ast::MemoryWidth::U8) => Ok("strb"),
        Some(crate::ast::MemoryWidth::I16 | crate::ast::MemoryWidth::U16) => Ok("strh"),
        Some(crate::ast::MemoryWidth::I32 | crate::ast::MemoryWidth::U32) => Ok("str"),
        Some(
            crate::ast::MemoryWidth::I64
            | crate::ast::MemoryWidth::U64
            | crate::ast::MemoryWidth::Ptr,
        ) => Ok("str"),
        _ => unsupported("integer memory store width"),
    }
}

fn integer_load_opcode(width: Option<crate::ast::MemoryWidth>) -> Result<&'static str, String> {
    match width {
        Some(crate::ast::MemoryWidth::I8) => Ok("ldrsb"),
        Some(crate::ast::MemoryWidth::U8) => Ok("ldrb"),
        Some(crate::ast::MemoryWidth::I16) => Ok("ldrsh"),
        Some(crate::ast::MemoryWidth::U16) => Ok("ldrh"),
        Some(crate::ast::MemoryWidth::I32) => Ok("ldrsw"),
        Some(crate::ast::MemoryWidth::U32) => Ok("ldr"),
        Some(
            crate::ast::MemoryWidth::I64
            | crate::ast::MemoryWidth::U64
            | crate::ast::MemoryWidth::Ptr,
        ) => Ok("ldr"),
        _ => unsupported("integer memory load width"),
    }
}

fn memory_register(register: &str, width: Option<crate::ast::MemoryWidth>) -> String {
    if width.is_some_and(|width| {
        matches!(
            width,
            crate::ast::MemoryWidth::I8
                | crate::ast::MemoryWidth::I16
                | crate::ast::MemoryWidth::I32
        )
    }) && register.starts_with('x')
    {
        register.to_owned()
    } else {
        narrow_register(register, width)
    }
}

fn integer_opcode(op: MathOp) -> Result<&'static str, String> {
    match op {
        MathOp::Add => Ok("add"),
        MathOp::BitAnd => Ok("and"),
        MathOp::BitOr => Ok("orr"),
        MathOp::BitXor => Ok("eor"),
        MathOp::Subtract => Ok("sub"),
        MathOp::Multiply => Ok("mul"),
        MathOp::ShiftLeft => Ok("lsl"),
        MathOp::ShiftRightArithmetic => Ok("asr"),
        MathOp::ShiftRightLogical => Ok("lsr"),
        _ => unsupported("integer operation"),
    }
}

fn bitwise_unary_opcode(op: BitwiseUnaryOp) -> &'static str {
    match op {
        BitwiseUnaryOp::Not => "mvn",
    }
}

fn compare_opcode(op: CompareOp, branch_when_true: bool) -> Result<&'static str, String> {
    let opcode = match op {
        CompareOp::Equal => "eq",
        CompareOp::NotEqual => "ne",
        CompareOp::SignedLess => "lt",
        CompareOp::SignedLessEqual => "le",
        CompareOp::SignedGreater => "gt",
        CompareOp::SignedGreaterEqual => "ge",
        CompareOp::UnsignedLess => "lo",
        CompareOp::UnsignedLessEqual => "ls",
        CompareOp::UnsignedGreater => "hi",
        CompareOp::UnsignedGreaterEqual => "hs",
        _ => return unsupported("comparison"),
    };
    if branch_when_true {
        Ok(opcode)
    } else {
        Ok(match opcode {
            "eq" => "ne",
            "ne" => "eq",
            "lt" => "ge",
            "le" => "gt",
            "gt" => "le",
            "ge" => "lt",
            "lo" => "hs",
            "ls" => "hi",
            "hi" => "ls",
            "hs" => "lo",
            _ => unreachable!(),
        })
    }
}

fn unsupported<T>(feature: &str) -> Result<T, String> {
    Err(format!("AArch64 backend does not support {feature} yet"))
}

#[cfg(test)]
#[path = "codegen_tests.rs"]
mod tests;
