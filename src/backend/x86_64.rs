use crate::analysis::StackFrame;
use crate::analysis::Width;
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

pub(crate) fn width(name: &str) -> Option<Width> {
    Some(match name {
        "rax" | "rbx" | "rcx" | "rdx" | "rdi" | "rsi" | "rbp" | "rsp" | "r8" | "r9" | "r10"
        | "r11" | "r12" | "r13" | "r14" | "r15" => Width::Bits64,
        "eax" | "ebx" | "ecx" | "edx" | "edi" | "esi" | "ebp" | "esp" | "r8d" | "r9d" | "r10d"
        | "r11d" | "r12d" | "r13d" | "r14d" | "r15d" => Width::Bits32,
        "ax" | "bx" | "cx" | "dx" | "di" | "si" | "bp" | "sp" | "r8w" | "r9w" | "r10w" | "r11w"
        | "r12w" | "r13w" | "r14w" | "r15w" => Width::Bits16,
        "al" | "bl" | "cl" | "dl" | "ah" | "bh" | "ch" | "dh" | "dil" | "sil" | "bpl" | "spl"
        | "r8b" | "r9b" | "r10b" | "r11b" | "r12b" | "r13b" | "r14b" | "r15b" => Width::Bits8,
        _ => return None,
    })
}

pub(crate) fn is_register(name: &str) -> bool {
    width(name).is_some() || is_xmm(name)
}

pub(crate) fn is_xmm(name: &str) -> bool {
    matches!(
        name,
        "xmm0"
            | "xmm1"
            | "xmm2"
            | "xmm3"
            | "xmm4"
            | "xmm5"
            | "xmm6"
            | "xmm7"
            | "xmm8"
            | "xmm9"
            | "xmm10"
            | "xmm11"
            | "xmm12"
            | "xmm13"
            | "xmm14"
            | "xmm15"
    )
}

pub(crate) fn family(name: &str) -> Option<&'static str> {
    Some(match name {
        "rax" | "eax" | "ax" | "al" | "ah" => "rax",
        "rbx" | "ebx" | "bx" | "bl" | "bh" => "rbx",
        "rcx" | "ecx" | "cx" | "cl" | "ch" => "rcx",
        "rdx" | "edx" | "dx" | "dl" | "dh" => "rdx",
        "rdi" | "edi" | "di" | "dil" => "rdi",
        "rsi" | "esi" | "si" | "sil" => "rsi",
        "rbp" | "ebp" | "bp" | "bpl" => "rbp",
        "rsp" | "esp" | "sp" | "spl" => "rsp",
        "r8" | "r8d" | "r8w" | "r8b" => "r8",
        "r9" | "r9d" | "r9w" | "r9b" => "r9",
        "r10" | "r10d" | "r10w" | "r10b" => "r10",
        "r11" | "r11d" | "r11w" | "r11b" => "r11",
        "r12" | "r12d" | "r12w" | "r12b" => "r12",
        "r13" | "r13d" | "r13w" | "r13b" => "r13",
        "r14" | "r14d" | "r14w" | "r14b" => "r14",
        "r15" | "r15d" | "r15w" | "r15b" => "r15",
        _ => return None,
    })
}

pub(crate) fn is_high_byte(name: &str) -> bool {
    matches!(name, "ah" | "bh" | "ch" | "dh")
}

pub(crate) fn is_extended(name: &str) -> bool {
    matches!(
        name,
        "r8" | "r9"
            | "r10"
            | "r11"
            | "r12"
            | "r13"
            | "r14"
            | "r15"
            | "r8d"
            | "r9d"
            | "r10d"
            | "r11d"
            | "r12d"
            | "r13d"
            | "r14d"
            | "r15d"
            | "r8w"
            | "r9w"
            | "r10w"
            | "r11w"
            | "r12w"
            | "r13w"
            | "r14w"
            | "r15w"
            | "r8b"
            | "r9b"
            | "r10b"
            | "r11b"
            | "r12b"
            | "r13b"
            | "r14b"
            | "r15b"
    )
}

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
    crate::machine::emit(
        &crate::machine::Instruction::Push {
            src: crate::machine::Operand::Register(spec.frame_pointer.to_owned()),
        },
        asm,
    );
    crate::machine::emit(
        &crate::machine::Instruction::Move {
            dst: crate::machine::Operand::Register(spec.frame_pointer.to_owned()),
            src: crate::machine::Operand::Register(spec.stack_pointer.to_owned()),
        },
        asm,
    );
    if stack.size > 0 {
        crate::machine::emit(
            &crate::machine::Instruction::StackAdjust {
                opcode: String::from("sub"),
                register: spec.stack_pointer.to_owned(),
                amount: stack.size,
            },
            asm,
        );
    }
}

pub(crate) fn emit_frame_epilogue(asm: &mut String, spec: TargetSpec) {
    crate::machine::emit(
        &crate::machine::Instruction::Move {
            dst: crate::machine::Operand::Register(spec.stack_pointer.to_owned()),
            src: crate::machine::Operand::Register(spec.frame_pointer.to_owned()),
        },
        asm,
    );
    crate::machine::emit(
        &crate::machine::Instruction::Pop {
            dst: crate::machine::Operand::Register(spec.frame_pointer.to_owned()),
        },
        asm,
    );
}
