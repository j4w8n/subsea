use crate::ast::{CompareOp, MathOp};
use crate::ir;
use std::collections::HashMap;

pub fn is_register(name: &str) -> bool {
    matches!(name, "sp" | "wsp")
        || (name.len() >= 2
            && matches!(&name[..1], "x" | "w" | "v" | "q" | "d" | "s" | "h" | "b")
            && name[1..].parse::<u8>().is_ok_and(|index| match &name[..1] {
                "x" | "w" => index <= 30,
                _ => index <= 31,
            }))
}

pub fn emit(program: &ir::Program) -> Result<String, String> {
    let mut asm = String::new();
    emit_data(&mut asm, program)?;
    asm.push_str(".text\n.global _start\n\n");

    for label in &program.labels {
        let slots = stack_slots(&label.stack);
        let frame_size = if slots.is_empty() { 0 } else { 16 };
        asm.push_str(&format!(
            "{}:\n",
            if label.name == program.entry {
                "_start"
            } else {
                &label.name
            }
        ));
        if frame_size > 0 {
            asm.push_str(&format!("  sub sp, sp, #{frame_size}\n"));
        }
        for (index, instruction) in label.instructions.iter().enumerate() {
            emit_instruction(&mut asm, instruction, &slots, frame_size).map_err(|message| {
                format!("__SUBSEA_AARCH__{}\0{}\0{message}", label.name, index)
            })?;
        }
        if frame_size > 0
            && !label
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, ir::Instruction::Ret))
        {
            asm.push_str(&format!("  add sp, sp, #{frame_size}\n"));
        }
    }

    Ok(asm)
}

fn emit_data(asm: &mut String, program: &ir::Program) -> Result<(), String> {
    if !program.data.is_empty() {
        asm.push_str(".section .rodata\n");
        for declaration in &program.data {
            if let Some(align) = declaration.align {
                asm.push_str(&format!(".balign {align}\n"));
            }
            if declaration.export {
                asm.push_str(&format!(".global {}\n", declaration.name));
            }
            asm.push_str(&format!("{}:\n", declaration.name));
            for item in &declaration.items {
                match item {
                    ir::DataItem::Scalar { width, value } => {
                        asm.push_str(&format!("  {} {value}\n", data_directive(*width)?));
                    }
                    ir::DataItem::Address { target } => {
                        asm.push_str(&format!("  .quad {target}\n"))
                    }
                    ir::DataItem::Zero { count } => asm.push_str(&format!("  .zero {count}\n")),
                    ir::DataItem::Label { name } => asm.push_str(&format!("{name}:\n")),
                }
            }
        }
    }

    for memory in &program.memory {
        match memory {
            ir::MemoryDeclaration::Buffer { name, width, count } => {
                asm.push_str(".section .bss\n");
                asm.push_str(&format!(
                    "{name}:\n  .zero {}\n",
                    width_size(*width) * count
                ));
            }
            ir::MemoryDeclaration::Scalar { name, width, value } => {
                asm.push_str(".section .data\n");
                asm.push_str(&format!("{name}:\n  {} {value}\n", data_directive(*width)?));
            }
            ir::MemoryDeclaration::Array {
                name,
                width,
                values,
            } => {
                asm.push_str(".section .data\n");
                asm.push_str(&format!("{name}:\n"));
                for value in values {
                    emit_memory_value(asm, *width, value)?;
                }
            }
            ir::MemoryDeclaration::Repeat {
                name,
                width,
                count,
                value,
            } => {
                asm.push_str(".section .data\n");
                asm.push_str(&format!("{name}:\n"));
                for _ in 0..*count {
                    emit_memory_value(asm, *width, value)?;
                }
            }
            ir::MemoryDeclaration::FloatScalar { .. } => {
                return unsupported("floating-point static data");
            }
        }
    }
    Ok(())
}

fn emit_memory_value(
    asm: &mut String,
    width: crate::ast::MemoryWidth,
    value: &ir::MemoryValue,
) -> Result<(), String> {
    match value {
        ir::MemoryValue::Integer(value) => {
            asm.push_str(&format!("  {} {value}\n", data_directive(width)?))
        }
        ir::MemoryValue::Address { target } => asm.push_str(&format!("  .quad {target}\n")),
    }
    Ok(())
}

fn data_directive(width: crate::ast::MemoryWidth) -> Result<&'static str, String> {
    match width {
        crate::ast::MemoryWidth::I8 | crate::ast::MemoryWidth::U8 => Ok(".byte"),
        crate::ast::MemoryWidth::I16 | crate::ast::MemoryWidth::U16 => Ok(".hword"),
        crate::ast::MemoryWidth::I32 | crate::ast::MemoryWidth::U32 => Ok(".word"),
        crate::ast::MemoryWidth::I64
        | crate::ast::MemoryWidth::U64
        | crate::ast::MemoryWidth::Ptr => Ok(".quad"),
        _ => unsupported("floating-point data directive"),
    }
}

fn width_size(width: crate::ast::MemoryWidth) -> usize {
    match width {
        crate::ast::MemoryWidth::I8 | crate::ast::MemoryWidth::U8 => 1,
        crate::ast::MemoryWidth::I16 | crate::ast::MemoryWidth::U16 => 2,
        crate::ast::MemoryWidth::I32 | crate::ast::MemoryWidth::U32 => 4,
        _ => 8,
    }
}

fn emit_instruction(
    asm: &mut String,
    instruction: &ir::Instruction,
    slots: &HashMap<String, usize>,
    frame_size: usize,
) -> Result<(), String> {
    match instruction {
        ir::Instruction::Assign { dst, value } => emit_assignment(asm, dst, value, slots),
        ir::Instruction::AssignIf {
            dst,
            value,
            condition,
        } => {
            let ir::Operand::TargetRegister(dst) = dst else {
                return unsupported("conditional assignment destination");
            };
            let ir::Value::Operand(value) = value else {
                return unsupported("conditional assignment value");
            };
            let ir::Operand::Immediate(value) = value else {
                return unsupported("conditional assignment value");
            };
            let skip = ".L.__subsea.aarch64.assign_if_skip";
            emit_condition_branch(asm, condition, skip, false, slots)?;
            asm.push_str(&format!("  mov {dst}, #{value}\n"));
            asm.push_str(&format!("{skip}:\n"));
            Ok(())
        }
        ir::Instruction::Call { target } => {
            match target {
                ir::ControlTarget::Label(target) => asm.push_str(&format!("  bl {target}\n")),
                ir::ControlTarget::Operand(ir::Operand::TargetRegister(register)) => {
                    asm.push_str(&format!("  blr {register}\n"));
                }
                _ => return unsupported("indirect calls"),
            }
            Ok(())
        }
        ir::Instruction::Exit { code } => {
            asm.push_str(&format!("  mov x0, #{code}\n  mov x8, #93\n  svc #0\n"));
            Ok(())
        }
        ir::Instruction::Jmp { target, condition } => {
            let ir::ControlTarget::Label(target) = target else {
                return unsupported("indirect jumps");
            };
            if let Some(condition) = condition {
                emit_condition_branch(asm, condition, target, true, slots)
            } else {
                asm.push_str(&format!("  b {target}\n"));
                Ok(())
            }
        }
        ir::Instruction::Label { name } => {
            asm.push_str(&format!("{name}:\n"));
            Ok(())
        }
        ir::Instruction::Nop => {
            asm.push_str("  nop\n");
            Ok(())
        }
        ir::Instruction::Runtime(operation) => emit_runtime(asm, operation),
        ir::Instruction::Ret => {
            if frame_size > 0 {
                asm.push_str(&format!("  add sp, sp, #{frame_size}\n"));
            }
            asm.push_str("  ret\n");
            Ok(())
        }
        ir::Instruction::Stack { name, width, value } => {
            let dst = stack_operand(name, Some(*width), slots)?;
            emit_assignment(asm, &dst, &ir::Value::Operand(value.clone()), slots)
        }
        ir::Instruction::Const { .. } => Ok(()),
        ir::Instruction::StackString { .. } => unsupported("stack strings"),
    }
}

fn emit_runtime(asm: &mut String, operation: &ir::RuntimeOperation) -> Result<(), String> {
    match operation {
        ir::RuntimeOperation::Print { parts } => {
            for part in parts {
                let ir::PrintPart::Literal(value) = part else {
                    return unsupported("formatted runtime printing");
                };
                let label = format!(".L.__subsea.aarch64.string_{}", asm.len());
                let bytes = value
                    .as_bytes()
                    .iter()
                    .map(|byte| byte.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                asm.push_str(".section .rodata\n");
                asm.push_str(&format!("{label}:\n  .byte {bytes}\n"));
                asm.push_str(".text\n");
                asm.push_str(&format!(
                    "  mov x0, #1\n  adrp x1, {label}\n  add x1, x1, :lo12:{label}\n  mov x2, #{}\n  mov x8, #64\n  svc #0\n",
                    value.len()
                ));
            }
            Ok(())
        }
        ir::RuntimeOperation::Read {
            source: ir::ReadSource::Stdin,
            dst: ir::Operand::TargetRegister(dst),
            len,
        } => {
            let len = operand(len, &HashMap::new())?;
            asm.push_str(&format!(
                "  mov x0, #0\n  mov x1, {dst}\n  mov x2, {len}\n  mov x8, #63\n  svc #0\n"
            ));
            Ok(())
        }
        ir::RuntimeOperation::Read { .. } => unsupported("memory-backed runtime input"),
        ir::RuntimeOperation::Release { .. } => unsupported("runtime memory release"),
    }
}

fn emit_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    value: &ir::Value,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    if let ir::Operand::Memory { address, width } = dst {
        let address = memory_address(address)?;
        let ir::Value::Operand(src) = value else {
            return unsupported("memory assignment value");
        };
        let src = operand(src, slots)?;
        if src.starts_with('#') {
            asm.push_str(&format!("  mov x16, {src}\n"));
            asm.push_str(&format!(
                "  str {}, {address}\n",
                narrow_register("x16", *width)
            ));
        } else {
            asm.push_str(&format!(
                "  str {}, {address}\n",
                narrow_register(&src, *width)
            ));
        }
        return Ok(());
    }

    let ir::Operand::TargetRegister(dst) = dst else {
        return unsupported("assignment destination");
    };

    match value {
        ir::Value::Operand(ir::Operand::Immediate(value)) => {
            asm.push_str(&format!("  mov {dst}, #{value}\n"));
        }
        ir::Value::Operand(ir::Operand::TargetRegister(src)) => {
            asm.push_str(&format!("  mov {dst}, {src}\n"));
        }
        ir::Value::Operand(ir::Operand::Memory { address, width }) => {
            asm.push_str(&format!(
                "  ldr {}, {}\n",
                narrow_register(dst, *width),
                memory_address(address)?
            ));
        }
        ir::Value::Binary { op, lhs, rhs } => {
            let lhs = operand(lhs, slots)?;
            let rhs = operand(rhs, slots)?;
            let opcode = integer_opcode(*op)?;
            asm.push_str(&format!("  {opcode} {dst}, {lhs}, {rhs}\n"));
        }
        _ => return unsupported("assignment value"),
    }
    Ok(())
}

fn emit_condition_branch(
    asm: &mut String,
    condition: &ir::Condition,
    target: &str,
    branch_when_true: bool,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    let ir::Condition::Compare { lhs, op, rhs } = condition else {
        return unsupported("bitwise conditions");
    };
    let lhs = operand(lhs, slots)?;
    let rhs = operand(rhs, slots)?;
    asm.push_str(&format!("  cmp {lhs}, {rhs}\n"));
    let opcode = compare_opcode(*op, branch_when_true)?;
    asm.push_str(&format!("  {opcode} {target}\n"));
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
        _ => unsupported("operand"),
    }
}

fn stack_slots(layout: &ir::StackLayout) -> HashMap<String, usize> {
    layout
        .slots
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            let name = match slot {
                ir::StackSlot::Scalar { name, .. } | ir::StackSlot::String { name } => name,
            };
            (name.clone(), index * 8)
        })
        .collect()
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
            first: ir::AddressTerm::TargetRegister(String::from("sp")),
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

fn memory_address(address: &ir::Address) -> Result<String, String> {
    let mut terms = vec![address_term(&address.first)?];
    for (operator, term) in &address.rest {
        if *operator == ir::AddressOperator::Subtract {
            return unsupported("negative address terms");
        }
        terms.push(address_term(term)?);
    }
    let Some(base) = terms.first() else {
        return unsupported("empty address");
    };
    if terms.len() == 1 {
        return Ok(format!("[{base}]"));
    }
    Ok(format!("[{}, {}]", base, terms[1..].join(", ")))
}

fn address_term(term: &ir::AddressTerm) -> Result<String, String> {
    match term {
        ir::AddressTerm::TargetRegister(register) => Ok(register.clone()),
        ir::AddressTerm::Immediate(value) => Ok(format!("#{value}")),
        ir::AddressTerm::ScaledTargetRegister { register, scale } => {
            if !matches!(scale, 1 | 2 | 4 | 8) {
                return unsupported("unsupported address scale");
            }
            let shift = match scale {
                1 => 0,
                2 => 1,
                4 => 2,
                8 => 3,
                _ => unreachable!(),
            };
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

fn integer_opcode(op: MathOp) -> Result<&'static str, String> {
    match op {
        MathOp::Add => Ok("add"),
        MathOp::Subtract => Ok("sub"),
        MathOp::Multiply => Ok("mul"),
        _ => unsupported("integer operation"),
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
