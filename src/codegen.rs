use crate::ast::{
    Address, AddressOperator, AddressTerm, Instruction, MemoryWidth, Operand, Program,
};

pub fn emit_x86_64_linux_asm(program: &Program) -> Result<String, String> {
    let mut asm = String::new();

    asm.push_str(".intel_syntax noprefix\n");
    asm.push_str(".global _start\n\n");
    asm.push_str("_start:\n");
    asm.push_str(&format!("  jmp {}\n\n", program.entry));

    for label in &program.labels {
        asm.push_str(&format!("{}:\n", label.name));

        for instruction in &label.instructions {
            match instruction {
                Instruction::Add { src, dst } => {
                    emit_binary_instruction(&mut asm, "add", src, dst)?;
                }
                Instruction::Copy { src, dst } => {
                    emit_copy_instruction(&mut asm, src, dst)?;
                }
                Instruction::Idiv { divisor } => {
                    let divisor = emit_operand(divisor)?;
                    asm.push_str(&format!("  idiv {divisor}\n"));
                }
                Instruction::Imul { src, dst } => {
                    emit_binary_instruction(&mut asm, "imul", src, dst)?;
                }
                Instruction::Jmp { target } => {
                    asm.push_str(&format!("  jmp {target}\n"));
                }
                Instruction::Sub { src, dst } => {
                    emit_binary_instruction(&mut asm, "sub", src, dst)?;
                }
                Instruction::Syscall => asm.push_str("  syscall\n"),
                Instruction::Udiv { divisor } => {
                    let divisor = emit_operand(divisor)?;
                    asm.push_str(&format!("  div {divisor}\n"));
                }
                Instruction::Umul { src, dst } => {
                    emit_binary_instruction(&mut asm, "imul", src, dst)?;
                }
            }
        }

        asm.push('\n');
    }

    Ok(asm)
}

fn emit_binary_instruction(
    asm: &mut String,
    opcode: &str,
    src: &Operand,
    dst: &Operand,
) -> Result<(), String> {
    validate_binary_operands(opcode, src, dst)?;

    let src = emit_operand(src)?;
    let dst = emit_operand(dst)?;
    asm.push_str(&format!("  {opcode} {dst}, {src}\n"));

    Ok(())
}

fn emit_copy_instruction(asm: &mut String, src: &Operand, dst: &Operand) -> Result<(), String> {
    if let Operand::Pointer(name) = src {
        validate_address_copy_dst(dst)?;

        let dst = emit_operand(dst)?;
        asm.push_str(&format!("  lea {dst}, [rip + {name}]\n"));

        Ok(())
    } else {
        emit_binary_instruction(asm, "mov", src, dst)
    }
}

fn validate_binary_operands(opcode: &str, src: &Operand, dst: &Operand) -> Result<(), String> {
    if matches!(
        dst,
        Operand::Immediate(_) | Operand::Ident(_) | Operand::Pointer(_)
    ) {
        return Err(format!(
            "{opcode} destination must be a register or memory operand"
        ));
    }

    if matches!(src, Operand::Pointer(_)) {
        return Err(format!("{opcode} source cannot be an address-of operand"));
    }

    if matches!(src, Operand::Dereference { .. }) && matches!(dst, Operand::Dereference { .. }) {
        return Err(format!(
            "{opcode} cannot use memory for both source and destination"
        ));
    }

    if opcode == "mov"
        && matches!(src, Operand::Immediate(_))
        && matches!(
            dst,
            Operand::Dereference {
                address: _,
                width: None
            }
        )
    {
        return Err(String::from(
            "Cannot copy an immediate value directly into memory without an explicit width",
        ));
    }

    if let (Some(src_width), Some(dst_width)) = (operand_width(src), operand_width(dst)) {
        if src_width != dst_width {
            return Err(format!(
                "Cannot use {}-bit source with {}-bit destination",
                src_width.bits(),
                dst_width.bits()
            ));
        }
    }

    Ok(())
}

fn validate_address_copy_dst(dst: &Operand) -> Result<(), String> {
    match dst {
        Operand::Register(_) => Ok(()),
        _ => Err(String::from(
            "Address-of labels can only be copied into registers for now",
        )),
    }
}

fn emit_operand(operand: &Operand) -> Result<String, String> {
    match operand {
        Operand::Dereference { address, width } => {
            let address = emit_address(address);

            Ok(match width {
                Some(width) => format!("{} ptr [{}]", memory_width_ptr(width), address),
                None => format!("[{address}]"),
            })
        }
        Operand::Immediate(value) => Ok(value.to_string()),
        Operand::Register(name) => Ok(name.clone()),
        Operand::Ident(name) => Ok(name.clone()),
        Operand::Pointer(name) => Err(format!(
            "Pointer operand &{name} is only supported as the source of copy"
        )),
    }
}

fn emit_address(address: &Address) -> String {
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

fn operand_width(operand: &Operand) -> Option<Width> {
    match operand {
        Operand::Register(name) => register_width(name),
        Operand::Dereference { width, .. } => width.as_ref().map(memory_width_bits),
        _ => None,
    }
}

fn memory_width_bits(width: &MemoryWidth) -> Width {
    match width {
        MemoryWidth::I8 | MemoryWidth::U8 => Width::Bits8,
        MemoryWidth::I16 | MemoryWidth::U16 => Width::Bits16,
        MemoryWidth::I32 | MemoryWidth::U32 => Width::Bits32,
        MemoryWidth::I64 | MemoryWidth::U64 => Width::Bits64,
    }
}

fn memory_width_ptr(width: &MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::I8 | MemoryWidth::U8 => "byte",
        MemoryWidth::I16 | MemoryWidth::U16 => "word",
        MemoryWidth::I32 | MemoryWidth::U32 => "dword",
        MemoryWidth::I64 | MemoryWidth::U64 => "qword",
    }
}

fn register_width(name: &str) -> Option<Width> {
    match name {
        "rax" | "rbx" | "rcx" | "rdx" | "rdi" | "rsi" | "rbp" | "rsp" | "r8" | "r9" | "r10"
        | "r11" | "r12" | "r13" | "r14" | "r15" => Some(Width::Bits64),
        "eax" | "ebx" | "ecx" | "edx" | "edi" | "esi" | "ebp" | "esp" | "r8d" | "r9d" | "r10d"
        | "r11d" | "r12d" | "r13d" | "r14d" | "r15d" => Some(Width::Bits32),
        "ax" | "bx" | "cx" | "dx" | "di" | "si" | "bp" | "sp" | "r8w" | "r9w" | "r10w" | "r11w"
        | "r12w" | "r13w" | "r14w" | "r15w" => Some(Width::Bits16),
        "al" | "bl" | "cl" | "dl" | "ah" | "bh" | "ch" | "dh" | "dil" | "sil" | "bpl" | "spl"
        | "r8b" | "r9b" | "r10b" | "r11b" | "r12b" | "r13b" | "r14b" | "r15b" => Some(Width::Bits8),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Width {
    Bits8,
    Bits16,
    Bits32,
    Bits64,
}

impl Width {
    fn bits(&self) -> u8 {
        match self {
            Width::Bits8 => 8,
            Width::Bits16 => 16,
            Width::Bits32 => 32,
            Width::Bits64 => 64,
        }
    }
}
