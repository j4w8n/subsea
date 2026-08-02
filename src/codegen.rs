use crate::ast::{Instruction, Operand, Program};

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
                    emit_binary_instruction(&mut asm, "mov", src, dst)?;
                }
                Instruction::Div { divisor } => {
                    let divisor = emit_operand(divisor)?;
                    asm.push_str(&format!("  div {divisor}\n"));
                }
                Instruction::Jmp { target } => {
                    asm.push_str(&format!("  jmp {target}\n"));
                }
                Instruction::Mul { src, dst } => {
                    emit_binary_instruction(&mut asm, "imul", src, dst)?;
                }
                Instruction::Sub { src, dst } => {
                    emit_binary_instruction(&mut asm, "sub", src, dst)?;
                }
                Instruction::Syscall => asm.push_str("  syscall\n"),
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
    let src = emit_operand(src)?;
    let dst = emit_operand(dst)?;
    asm.push_str(&format!("  {opcode} {dst}, {src}\n"));

    Ok(())
}

fn emit_operand(operand: &Operand) -> Result<String, String> {
    match operand {
        Operand::Immediate(value) => Ok(value.to_string()),
        Operand::Register(name) => Ok(name.clone()),
        Operand::Ident(name) => Ok(name.clone()),
        Operand::Pointer(name) => Err(format!("Pointer operand &{name} is not supported yet")),
    }
}
