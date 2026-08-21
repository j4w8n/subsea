//! x86-64 assembly syntax and operand formatting.
//!
//! This module owns assembler spelling only. Instruction selection and runtime
//! policy remain in `codegen`.

use std::fmt::{Display, Formatter, Result as FmtResult, Write};

#[derive(Debug, PartialEq, Clone)]
pub enum Operand {
    Immediate(i128),
    Register(String),
    Memory(MemoryAddress),
    Address(String),
}

#[derive(Debug, PartialEq, Clone)]
pub struct MemoryAddress {
    pub width: Option<String>,
    pub terms: Vec<(AddressOperator, AddressTerm)>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AddressOperator {
    Add,
    Subtract,
}

#[derive(Debug, PartialEq, Clone)]
pub enum AddressTerm {
    Immediate(i128),
    Symbol(String),
    Register(String),
    ScaledRegister { register: String, scale: i64 },
}

impl Display for Operand {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Immediate(value) => value.fmt(formatter),
            Self::Register(name) | Self::Address(name) => name.fmt(formatter),
            Self::Memory(address) => address.fmt(formatter),
        }
    }
}

impl Display for MemoryAddress {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        if let Some(width) = &self.width {
            write!(formatter, "{width} ptr ")?;
        }
        formatter.write_str("[")?;
        for (index, (operator, term)) in self.terms.iter().enumerate() {
            if index > 0 {
                formatter.write_str(match operator {
                    AddressOperator::Add => " + ",
                    AddressOperator::Subtract => " - ",
                })?;
            }
            term.fmt(formatter)?;
        }
        formatter.write_str("]")
    }
}

impl Display for AddressTerm {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Immediate(value) => value.fmt(formatter),
            Self::Symbol(name) | Self::Register(name) => name.fmt(formatter),
            Self::ScaledRegister { register, scale } => write!(formatter, "{register} * {scale}"),
        }
    }
}

pub fn instruction(asm: &mut String, text: impl Display) {
    let _ = writeln!(asm, "  {text}");
}

pub fn label(asm: &mut String, name: impl Display) {
    let _ = writeln!(asm, "{name}:");
}

pub fn nop(asm: &mut String) {
    instruction(asm, "nop");
}

pub fn mov(asm: &mut String, dst: Operand, src: Operand) {
    instruction(asm, format_args!("mov {dst}, {src}"));
}

pub fn load(asm: &mut String, dst: Operand, src: Operand) {
    mov(asm, dst, src);
}

pub fn store(asm: &mut String, dst: Operand, src: Operand) {
    mov(asm, dst, src);
}

pub fn lea(asm: &mut String, dst: Operand, address: impl Display) {
    instruction(asm, format_args!("lea {dst}, {address}"));
}

pub fn float_move(asm: &mut String, opcode: impl Display, dst: Operand, src: Operand) {
    instruction(asm, format_args!("{opcode} {dst}, {src}"));
}

pub fn float_binary(asm: &mut String, opcode: impl Display, dst: Operand, src: Operand) {
    instruction(asm, format_args!("{opcode} {dst}, {src}"));
}

pub fn binary(asm: &mut String, opcode: impl Display, dst: Operand, src: Operand) {
    instruction(asm, format_args!("{opcode} {dst}, {src}"));
}

pub fn unary(asm: &mut String, opcode: impl Display, operand: Operand) {
    instruction(asm, format_args!("{opcode} {operand}"));
}

pub fn compare(asm: &mut String, opcode: impl Display, lhs: Operand, rhs: Operand) {
    instruction(asm, format_args!("{opcode} {lhs}, {rhs}"));
}

pub fn call(asm: &mut String, target: Operand) {
    instruction(asm, format_args!("call {target}"));
}

pub fn branch(asm: &mut String, opcode: impl Display, target: Operand) {
    instruction(asm, format_args!("{opcode} {target}"));
}

pub fn jump(asm: &mut String, target: Operand) {
    instruction(asm, format_args!("jmp {target}"));
}

pub fn push(asm: &mut String, src: Operand) {
    instruction(asm, format_args!("push {src}"));
}

pub fn pop(asm: &mut String, dst: Operand) {
    instruction(asm, format_args!("pop {dst}"));
}

pub fn stack_adjust(asm: &mut String, opcode: impl Display, register: impl Display, amount: usize) {
    instruction(asm, format_args!("{opcode} {register}, {amount}"));
}

pub fn syscall(asm: &mut String, number: u64) {
    instruction(asm, format_args!("mov rax, {number}"));
    instruction(asm, "syscall");
}

pub fn syscall_trap(asm: &mut String) {
    instruction(asm, "syscall");
}

pub fn prepare_division(asm: &mut String, signed: bool) {
    instruction(asm, if signed { "cqo" } else { "xor rdx, rdx" });
}

pub fn divide(asm: &mut String, opcode: impl Display, divisor: Operand) {
    instruction(asm, format_args!("{opcode} {divisor}"));
}

pub fn wide_math(asm: &mut String, opcode: impl Display, operand: Operand) {
    instruction(asm, format_args!("{opcode} {operand}"));
}

pub fn ret(asm: &mut String) {
    instruction(asm, "ret");
}

pub fn intel_syntax(asm: &mut String) {
    top_level_directive(asm, ".intel_syntax noprefix");
}

pub fn section(asm: &mut String, name: &str) {
    top_level_directive(asm, format_args!(".section .{name}"));
}

pub fn text(asm: &mut String) {
    section(asm, "text");
}

pub fn global(asm: &mut String, name: &str) {
    top_level_directive(asm, format_args!(".global {name}"));
}

pub fn directive(asm: &mut String, text: impl Display) {
    let _ = writeln!(asm, "  {text}");
}

pub fn scalar(asm: &mut String, name: &str, value: impl Display) {
    directive(asm, format_args!("{name} {value}"));
}

pub fn byte(asm: &mut String, value: impl Display) {
    scalar(asm, ".byte", value);
}

pub fn quad(asm: &mut String, value: impl Display) {
    scalar(asm, ".quad", value);
}

pub fn zero(asm: &mut String, count: impl Display) {
    scalar(asm, ".zero", count);
}

pub fn top_level_directive(asm: &mut String, text: impl Display) {
    let _ = writeln!(asm, "{text}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_emit_basic_instructions() {
        let mut asm = String::new();

        mov(
            &mut asm,
            Operand::Register("rax".to_owned()),
            Operand::Immediate(1),
        );
        compare(
            &mut asm,
            "cmp",
            Operand::Register("rax".to_owned()),
            Operand::Immediate(0),
        );
        branch(&mut asm, "je", Operand::Address("done".to_owned()));
        ret(&mut asm);

        assert_eq!(asm, "  mov rax, 1\n  cmp rax, 0\n  je done\n  ret\n");
    }

    #[test]
    fn helpers_emit_directives_and_sequences() {
        let mut asm = String::new();

        intel_syntax(&mut asm);
        section(&mut asm, "rodata");
        global(&mut asm, "message");
        label(&mut asm, "message");
        byte(&mut asm, "1, 2, 3");
        quad(&mut asm, "target");
        zero(&mut asm, 8);
        syscall(&mut asm, 60);
        prepare_division(&mut asm, true);

        assert_eq!(
            asm,
            ".intel_syntax noprefix\n.section .rodata\n.global message\nmessage:\n  .byte 1, 2, 3\n  .quad target\n  .zero 8\n  mov rax, 60\n  syscall\n  cqo\n"
        );
    }
}
