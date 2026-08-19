use crate::analysis::StackFrame;
use crate::analysis::{
    StringTable, resolve_memory_width, stack_scalar_slot, stack_string_property_slot,
    stack_string_slot,
};
use crate::ast::StringProperty;
use crate::ast::{
    Address, AddressOperator, AddressTerm, BitwiseUnaryOp, CompareOp, FloatMathOp, IntrinsicOp,
    MathOp, MemoryWidth, Operand,
};
use crate::backend::TargetSpec;
use crate::ir;
use crate::platform::linux;

pub(crate) mod codegen;
pub mod machine;
mod registers;

pub(crate) use registers::width;
pub(crate) use registers::{family, is_extended, is_high_byte, is_register, is_vector, is_xmm};

pub(crate) fn emit_operand(
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<String, String> {
    match operand {
        Operand::Converted { .. } | Operand::Cast { .. } => Err(String::from(
            "Conversion operands are only supported as assignment sources",
        )),
        Operand::AddressOf(_) => Err(String::from(
            "Address-of operands are only supported as assignment sources",
        )),
        Operand::Dereference { address, width } => {
            let emitted_address = emit_address(address);

            Ok(match resolve_memory_width(address, *width, strings)? {
                Some(width) => format!("{} ptr [{}]", width.ptr(), emitted_address),
                None => format!("[{emitted_address}]"),
            })
        }
        Operand::FloatLiteral(value) => Err(format!(
            "Float literal {value} requires an explicit floating-point operator width"
        )),
        Operand::Immediate(value) => Ok(value.to_string()),
        Operand::Register(name) => Ok(name.clone()),
        Operand::Ident(name) => match stack_scalar_slot(stack, name) {
            Some((offset, width)) => Ok(format!("{} ptr [rbp - {}]", width.ptr(), offset)),
            None if stack_string_slot(stack, name).is_some() => Err(format!(
                "String stack variable {name:?} in label {label_name:?} cannot be used as an operand"
            )),
            None => match strings
                .integers
                .get(&(label_name.to_string(), name.clone()))
            {
                Some(binding) => Ok(binding.value.to_string()),
                None if strings
                    .float_bindings
                    .contains_key(&(label_name.to_string(), name.clone())) =>
                {
                    Err(format!(
                        "Float binding {name:?} in label {label_name:?} requires a floating-point operator width"
                    ))
                }
                None if strings
                    .bindings
                    .contains_key(&(label_name.to_string(), name.clone())) =>
                {
                    Err(format!(
                        "String binding {name:?} in label {label_name:?} cannot be used as an operand"
                    ))
                }
                None => Err(format!("Unknown binding {name:?} in label {label_name:?}")),
            },
        },
        Operand::StringProperty { name, property } => {
            if let Some(offset) = stack_string_property_slot(stack, name, *property) {
                return Ok(format!("qword ptr [rbp - {offset}]"));
            }

            let binding = strings
                .bindings
                .get(&(label_name.to_string(), name.clone()))
                .ok_or_else(|| {
                    format!("Unknown string binding {name:?} in label {label_name:?}")
                })?;

            Ok(match property {
                StringProperty::Len => binding.value.len().to_string(),
                StringProperty::Ptr => format!("offset {}", binding.asm_label),
            })
        }
        Operand::Pointer(name) => Err(format!(
            "Pointer operand &{name} is only supported as the right side of assignment"
        )),
    }
}

pub(crate) fn emit_ir_operand(
    operand: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<String, String> {
    match operand {
        ir::Operand::Immediate(value) => Ok(value.to_string()),
        ir::Operand::Name(name) => match stack_scalar_slot(stack, name) {
            Some((offset, width)) => Ok(format!("{} ptr [rbp - {}]", width.ptr(), offset)),
            None if stack_string_slot(stack, name).is_some() => Err(format!(
                "String stack variable {name:?} in label {label_name:?} cannot be used as an operand"
            )),
            None => match strings
                .integers
                .get(&(label_name.to_string(), name.clone()))
            {
                Some(binding) => Ok(binding.value.to_string()),
                None if strings
                    .float_bindings
                    .contains_key(&(label_name.to_string(), name.clone())) =>
                {
                    Err(format!(
                        "Float binding {name:?} in label {label_name:?} requires a floating-point operator width"
                    ))
                }
                None if strings
                    .bindings
                    .contains_key(&(label_name.to_string(), name.clone())) =>
                {
                    Err(format!(
                        "String binding {name:?} in label {label_name:?} cannot be used as an operand"
                    ))
                }
                None => Err(format!("Unknown binding {name:?} in label {label_name:?}")),
            },
        },
        ir::Operand::Memory { address, width } => {
            let emitted_address = emit_ir_address(address);
            let ast_address = ir_address_to_ast(address);
            Ok(match resolve_memory_width(&ast_address, *width, strings)? {
                Some(width) => format!("{} ptr [{}]", width.ptr(), emitted_address),
                None => format!("[{emitted_address}]"),
            })
        }
        ir::Operand::StringProperty { name, property } => {
            if let Some(offset) = stack_string_property_slot(
                stack,
                name,
                match property {
                    ir::StringProperty::Len => crate::ast::StringProperty::Len,
                    ir::StringProperty::Ptr => crate::ast::StringProperty::Ptr,
                },
            ) {
                return Ok(format!("qword ptr [rbp - {offset}]"));
            }

            let binding = strings
                .bindings
                .get(&(label_name.to_string(), name.clone()))
                .ok_or_else(|| {
                    format!("Unknown string binding {name:?} in label {label_name:?}")
                })?;

            Ok(match property {
                ir::StringProperty::Len => binding.value.len().to_string(),
                ir::StringProperty::Ptr => format!("offset {}", binding.asm_label),
            })
        }
        ir::Operand::TargetRegister(name) => Ok(name.clone()),
        ir::Operand::FloatLiteral(value) => Err(format!(
            "Float literal {value} requires an explicit floating-point operator width"
        )),
        ir::Operand::Pointer(name) => Err(format!(
            "Pointer operand &{name} is only supported as the right side of assignment"
        )),
        ir::Operand::AddressOf(_) => Err(String::from(
            "Address-of operands are only supported as assignment sources",
        )),
        ir::Operand::Converted { .. } | ir::Operand::Cast { .. } => Err(String::from(
            "Conversion operands are only supported as assignment sources",
        )),
    }
}

fn emit_ir_address(address: &ir::Address) -> String {
    let mut value = emit_ir_address_term(&address.first);

    for (operator, term) in &address.rest {
        value.push_str(match operator {
            ir::AddressOperator::Add => " + ",
            ir::AddressOperator::Subtract => " - ",
        });
        value.push_str(&emit_ir_address_term(term));
    }

    value
}

fn emit_ir_address_term(term: &ir::AddressTerm) -> String {
    match term {
        ir::AddressTerm::Immediate(value) => value.to_string(),
        ir::AddressTerm::Name(name) => name.clone(),
        ir::AddressTerm::TargetRegister(name) => name.clone(),
        ir::AddressTerm::ScaledTargetRegister { register, scale } => {
            format!("{register} * {scale}")
        }
    }
}

fn ir_address_to_ast(address: &ir::Address) -> Address {
    Address {
        first: match &address.first {
            ir::AddressTerm::Immediate(value) => AddressTerm::Immediate(*value),
            ir::AddressTerm::Name(name) => AddressTerm::Ident(name.clone()),
            ir::AddressTerm::TargetRegister(name) => AddressTerm::Register(name.clone()),
            ir::AddressTerm::ScaledTargetRegister { register, scale } => {
                AddressTerm::ScaledRegister {
                    register: register.clone(),
                    scale: *scale,
                }
            }
        },
        rest: address
            .rest
            .iter()
            .map(|(operator, term)| {
                (
                    match operator {
                        ir::AddressOperator::Add => AddressOperator::Add,
                        ir::AddressOperator::Subtract => AddressOperator::Subtract,
                    },
                    match term {
                        ir::AddressTerm::Immediate(value) => AddressTerm::Immediate(*value),
                        ir::AddressTerm::Name(name) => AddressTerm::Ident(name.clone()),
                        ir::AddressTerm::TargetRegister(name) => {
                            AddressTerm::Register(name.clone())
                        }
                        ir::AddressTerm::ScaledTargetRegister { register, scale } => {
                            AddressTerm::ScaledRegister {
                                register: register.clone(),
                                scale: *scale,
                            }
                        }
                    },
                )
            })
            .collect(),
    }
}

pub(crate) fn emit_address(address: &Address) -> String {
    let mut value = emit_address_term(&address.first);

    for (operator, term) in &address.rest {
        match operator {
            AddressOperator::Add => value.push_str(" + "),
            AddressOperator::Subtract => value.push_str(" - "),
        }

        value.push_str(&emit_address_term(term));
    }

    value
}

fn emit_address_term(term: &AddressTerm) -> String {
    match term {
        AddressTerm::Immediate(value) => value.to_string(),
        AddressTerm::Register(name) => name.clone(),
        AddressTerm::ScaledRegister { register, scale } => format!("{register} * {scale}"),
        AddressTerm::Ident(name) => name.clone(),
    }
}

pub(crate) fn integer_math_opcode(op: MathOp) -> &'static str {
    match op {
        MathOp::Add => "add",
        MathOp::BitAnd => "and",
        MathOp::BitOr => "or",
        MathOp::BitXor => "xor",
        MathOp::Multiply => "imul",
        MathOp::Power => unreachable!(),
        MathOp::ShiftLeft => "shl",
        MathOp::ShiftRightArithmetic => "sar",
        MathOp::ShiftRightLogical => "shr",
        MathOp::Subtract => "sub",
    }
}

pub(crate) fn bitwise_unary_opcode(op: BitwiseUnaryOp) -> &'static str {
    match op {
        BitwiseUnaryOp::Not => "not",
    }
}

pub(crate) fn compare_jump_opcode(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Equal => "je",
        CompareOp::NotEqual => "jne",
        CompareOp::SignedLess => "jl",
        CompareOp::SignedLessEqual => "jle",
        CompareOp::SignedGreater => "jg",
        CompareOp::SignedGreaterEqual => "jge",
        CompareOp::UnsignedLess => "jb",
        CompareOp::UnsignedLessEqual => "jbe",
        CompareOp::UnsignedGreater => "ja",
        CompareOp::UnsignedGreaterEqual => "jae",
        CompareOp::Less | CompareOp::LessEqual | CompareOp::Greater | CompareOp::GreaterEqual => {
            unreachable!()
        }
        CompareOp::FloatEqual(_)
        | CompareOp::FloatNotEqual(_)
        | CompareOp::FloatLess(_)
        | CompareOp::FloatLessEqual(_)
        | CompareOp::FloatGreater(_)
        | CompareOp::FloatGreaterEqual(_) => unreachable!(),
    }
}

pub(crate) fn compare_set_opcode(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Equal => "sete",
        CompareOp::NotEqual => "setne",
        CompareOp::SignedLess => "setl",
        CompareOp::SignedLessEqual => "setle",
        CompareOp::SignedGreater => "setg",
        CompareOp::SignedGreaterEqual => "setge",
        CompareOp::UnsignedLess => "setb",
        CompareOp::UnsignedLessEqual => "setbe",
        CompareOp::UnsignedGreater => "seta",
        CompareOp::UnsignedGreaterEqual => "setae",
        CompareOp::Less | CompareOp::LessEqual | CompareOp::Greater | CompareOp::GreaterEqual => {
            unreachable!()
        }
        CompareOp::FloatEqual(_)
        | CompareOp::FloatNotEqual(_)
        | CompareOp::FloatLess(_)
        | CompareOp::FloatLessEqual(_)
        | CompareOp::FloatGreater(_)
        | CompareOp::FloatGreaterEqual(_) => unreachable!(),
    }
}

pub(crate) fn float_sqrt_opcode(width: MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::F32 => "sqrtss",
        MemoryWidth::F64 => "sqrtsd",
        _ => unreachable!(),
    }
}

pub(crate) fn float_rounding_opcode(width: MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::F32 => "roundss",
        MemoryWidth::F64 => "roundsd",
        _ => unreachable!(),
    }
}

pub(crate) fn float_rounding_mode(op: IntrinsicOp) -> u8 {
    match op {
        IntrinsicOp::Round => 0,
        IntrinsicOp::Floor => 1,
        IntrinsicOp::Ceil => 2,
        IntrinsicOp::Trunc => 3,
        _ => unreachable!(),
    }
}

pub(crate) fn float_min_max_opcode(op: IntrinsicOp, width: MemoryWidth) -> &'static str {
    match (op, width) {
        (IntrinsicOp::Min, MemoryWidth::F32) => "minss",
        (IntrinsicOp::Min, MemoryWidth::F64) => "minsd",
        (IntrinsicOp::Max, MemoryWidth::F32) => "maxss",
        (IntrinsicOp::Max, MemoryWidth::F64) => "maxsd",
        _ => unreachable!(),
    }
}

pub(crate) fn float_math_opcode(op: FloatMathOp, width: MemoryWidth) -> &'static str {
    match (op, width) {
        (FloatMathOp::Add, MemoryWidth::F32) => "addss",
        (FloatMathOp::Add, MemoryWidth::F64) => "addsd",
        (FloatMathOp::Divide, MemoryWidth::F32) => "divss",
        (FloatMathOp::Divide, MemoryWidth::F64) => "divsd",
        (FloatMathOp::Multiply, MemoryWidth::F32) => "mulss",
        (FloatMathOp::Multiply, MemoryWidth::F64) => "mulsd",
        (FloatMathOp::Subtract, MemoryWidth::F32) => "subss",
        (FloatMathOp::Subtract, MemoryWidth::F64) => "subsd",
        _ => unreachable!(),
    }
}

pub(crate) fn float_move_opcode(width: MemoryWidth) -> Result<&'static str, String> {
    match width {
        MemoryWidth::F32 => Ok("movss"),
        MemoryWidth::F64 => Ok("movsd"),
        _ => Err(String::from(
            "XMM moves require an explicitly f32 or f64 memory operand",
        )),
    }
}

pub(crate) fn float_compare_opcode(width: MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::F32 => "ucomiss",
        MemoryWidth::F64 => "ucomisd",
        _ => unreachable!(),
    }
}

pub(crate) fn float_compare_jump_opcode(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Equal | CompareOp::FloatEqual(_) => "je",
        CompareOp::NotEqual | CompareOp::FloatNotEqual(_) => "jne",
        CompareOp::Less | CompareOp::FloatLess(_) => "jb",
        CompareOp::LessEqual | CompareOp::FloatLessEqual(_) => "jbe",
        CompareOp::Greater | CompareOp::FloatGreater(_) => "ja",
        CompareOp::GreaterEqual | CompareOp::FloatGreaterEqual(_) => "jae",
        _ => unreachable!(),
    }
}

pub(crate) fn division_opcode(signed: bool) -> &'static str {
    if signed { "idiv" } else { "div" }
}

pub(crate) fn wide_math_opcode(division: bool, signed: bool) -> &'static str {
    match (division, signed) {
        (false, true) => "imul",
        (false, false) => "mul",
        (true, true) => "idiv",
        (true, false) => "div",
    }
}

pub(crate) fn pair_math_opcodes(op: crate::ast::PairBinaryOp) -> (&'static str, &'static str) {
    match op {
        crate::ast::PairBinaryOp::Add => ("add", "adc"),
        crate::ast::PairBinaryOp::Subtract => ("sub", "sbb"),
    }
}

pub(crate) fn emit_frame_prologue(asm: &mut String, stack: &StackFrame, spec: TargetSpec) {
    machine::emit(
        &machine::Instruction::Push {
            src: machine::Operand::Register(spec.frame_pointer.to_owned()),
        },
        asm,
    );
    machine::emit(
        &machine::Instruction::Move {
            dst: machine::Operand::Register(spec.frame_pointer.to_owned()),
            src: machine::Operand::Register(spec.stack_pointer.to_owned()),
        },
        asm,
    );
    if stack.size > 0 {
        machine::emit(
            &machine::Instruction::StackAdjust {
                opcode: String::from("sub"),
                register: spec.stack_pointer.to_owned(),
                amount: stack.size,
            },
            asm,
        );
    }
}

pub(crate) fn emit_frame_epilogue(asm: &mut String, spec: TargetSpec) {
    machine::emit(
        &machine::Instruction::Move {
            dst: machine::Operand::Register(spec.stack_pointer.to_owned()),
            src: machine::Operand::Register(spec.frame_pointer.to_owned()),
        },
        asm,
    );
    machine::emit(
        &machine::Instruction::Pop {
            dst: machine::Operand::Register(spec.frame_pointer.to_owned()),
        },
        asm,
    );
}

pub(crate) fn emit_linux_syscall(asm: &mut String, number: u64) {
    machine::emit(&machine::Instruction::Syscall { number }, asm);
}

pub(crate) fn emit_linux_write_label(asm: &mut String, label: &str, len: usize) {
    machine::emit(
        &machine::Instruction::Move {
            dst: machine::Operand::Register(String::from("rax")),
            src: machine::Operand::Immediate(linux::SYS_WRITE as i128),
        },
        asm,
    );
    asm.push_str(&format!(
        "  mov rdi, {}\n  lea rsi, [rip + {label}]\n  mov rdx, {len}\n",
        linux::STDOUT
    ));
    machine::emit(&machine::Instruction::SyscallTrap, asm);
}

pub(crate) fn emit_linux_write_registers(asm: &mut String) {
    machine::emit(
        &machine::Instruction::Move {
            dst: machine::Operand::Register(String::from("rax")),
            src: machine::Operand::Immediate(linux::SYS_WRITE as i128),
        },
        asm,
    );
    asm.push_str(&format!("  mov rdi, {}\n", linux::STDOUT));
    machine::emit(&machine::Instruction::SyscallTrap, asm);
}

pub(crate) fn emit_linux_read(asm: &mut String) {
    emit_linux_syscall(asm, linux::SYS_READ);
}

pub(crate) fn emit_linux_mmap(asm: &mut String) {
    emit_linux_syscall(asm, linux::SYS_MMAP);
}

pub(crate) fn emit_linux_munmap(asm: &mut String) {
    emit_linux_syscall(asm, linux::SYS_MUNMAP);
}
