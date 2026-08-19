//! x86-64 assembly syntax and operand formatting.
//!
//! This module is the low-level assembly boundary for the x86-64 backend. The
//! instruction-shaped types remain temporarily for compatibility and for the
//! incremental migration away from the former `machine` module.

use std::fmt::{Display, Write};

#[derive(Debug, PartialEq, Clone)]
pub struct Program {
    pub instructions: Vec<Instruction>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Instruction {
    Label {
        name: String,
    },
    Nop,
    Move {
        dst: Operand,
        src: Operand,
    },
    Load {
        dst: Operand,
        src: Operand,
    },
    LoadAddress {
        dst: Operand,
        address: String,
    },
    Store {
        dst: Operand,
        src: Operand,
    },
    FloatMove {
        opcode: String,
        dst: Operand,
        src: Operand,
    },
    FloatBinary {
        opcode: String,
        dst: Operand,
        src: Operand,
    },
    Binary {
        opcode: String,
        dst: Operand,
        src: Operand,
    },
    Unary {
        opcode: String,
        operand: Operand,
    },
    Compare {
        opcode: String,
        lhs: Operand,
        rhs: Operand,
    },
    Call {
        target: Operand,
    },
    RuntimeCall {
        target: Operand,
    },
    Branch {
        opcode: String,
        target: Operand,
    },
    Jump {
        target: Operand,
    },
    Push {
        src: Operand,
    },
    Pop {
        dst: Operand,
    },
    StackAdjust {
        opcode: String,
        register: String,
        amount: usize,
    },
    Syscall {
        number: u64,
    },
    SyscallTrap,
    PrepareDivision {
        signed: bool,
    },
    Divide {
        opcode: String,
        divisor: Operand,
    },
    WideMath {
        opcode: String,
        operand: Operand,
    },
    Return,
}

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

pub fn instruction(asm: &mut String, text: impl Display) {
    let _ = writeln!(asm, "  {text}");
}

pub fn label(asm: &mut String, name: impl Into<String>) {
    emit(&Instruction::Label { name: name.into() }, asm);
}

pub fn nop(asm: &mut String) {
    emit(&Instruction::Nop, asm);
}

pub fn mov(asm: &mut String, dst: Operand, src: Operand) {
    emit(&Instruction::Move { dst, src }, asm);
}

pub fn load(asm: &mut String, dst: Operand, src: Operand) {
    emit(&Instruction::Load { dst, src }, asm);
}

pub fn store(asm: &mut String, dst: Operand, src: Operand) {
    emit(&Instruction::Store { dst, src }, asm);
}

pub fn lea(asm: &mut String, dst: Operand, address: impl Into<String>) {
    emit(
        &Instruction::LoadAddress {
            dst,
            address: address.into(),
        },
        asm,
    );
}

pub fn float_move(asm: &mut String, opcode: impl Into<String>, dst: Operand, src: Operand) {
    emit(
        &Instruction::FloatMove {
            opcode: opcode.into(),
            dst,
            src,
        },
        asm,
    );
}

pub fn float_binary(asm: &mut String, opcode: impl Into<String>, dst: Operand, src: Operand) {
    emit(
        &Instruction::FloatBinary {
            opcode: opcode.into(),
            dst,
            src,
        },
        asm,
    );
}

pub fn binary(asm: &mut String, opcode: impl Into<String>, dst: Operand, src: Operand) {
    emit(
        &Instruction::Binary {
            opcode: opcode.into(),
            dst,
            src,
        },
        asm,
    );
}

pub fn unary(asm: &mut String, opcode: impl Into<String>, operand: Operand) {
    emit(
        &Instruction::Unary {
            opcode: opcode.into(),
            operand,
        },
        asm,
    );
}

pub fn compare(asm: &mut String, opcode: impl Into<String>, lhs: Operand, rhs: Operand) {
    emit(
        &Instruction::Compare {
            opcode: opcode.into(),
            lhs,
            rhs,
        },
        asm,
    );
}

pub fn call(asm: &mut String, target: Operand) {
    emit(&Instruction::Call { target }, asm);
}

pub fn runtime_call(asm: &mut String, target: Operand) {
    emit(&Instruction::RuntimeCall { target }, asm);
}

pub fn branch(asm: &mut String, opcode: impl Into<String>, target: Operand) {
    emit(
        &Instruction::Branch {
            opcode: opcode.into(),
            target,
        },
        asm,
    );
}

pub fn jump(asm: &mut String, target: Operand) {
    emit(&Instruction::Jump { target }, asm);
}

pub fn push(asm: &mut String, src: Operand) {
    emit(&Instruction::Push { src }, asm);
}

pub fn pop(asm: &mut String, dst: Operand) {
    emit(&Instruction::Pop { dst }, asm);
}

pub fn stack_adjust(
    asm: &mut String,
    opcode: impl Into<String>,
    register: impl Into<String>,
    amount: usize,
) {
    emit(
        &Instruction::StackAdjust {
            opcode: opcode.into(),
            register: register.into(),
            amount,
        },
        asm,
    );
}

pub fn syscall(asm: &mut String, number: u64) {
    emit(&Instruction::Syscall { number }, asm);
}

pub fn syscall_trap(asm: &mut String) {
    emit(&Instruction::SyscallTrap, asm);
}

pub fn prepare_division(asm: &mut String, signed: bool) {
    emit(&Instruction::PrepareDivision { signed }, asm);
}

pub fn divide(asm: &mut String, opcode: impl Into<String>, divisor: Operand) {
    emit(
        &Instruction::Divide {
            opcode: opcode.into(),
            divisor,
        },
        asm,
    );
}

pub fn wide_math(asm: &mut String, opcode: impl Into<String>, operand: Operand) {
    emit(
        &Instruction::WideMath {
            opcode: opcode.into(),
            operand,
        },
        asm,
    );
}

pub fn ret(asm: &mut String) {
    emit(&Instruction::Return, asm);
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

pub fn emit(instruction: &Instruction, asm: &mut String) {
    match instruction {
        Instruction::Label { name } => asm.push_str(&format!("{name}:\n")),
        Instruction::Nop => asm.push_str("  nop\n"),
        Instruction::Move { dst, src } => {
            asm.push_str(&format!("  mov {}, {}\n", display(dst), display(src)));
        }
        Instruction::Load { dst, src } | Instruction::Store { dst, src } => {
            asm.push_str(&format!("  mov {}, {}\n", display(dst), display(src)));
        }
        Instruction::LoadAddress { dst, address } => {
            asm.push_str(&format!("  lea {}, {address}\n", display(dst)));
        }
        Instruction::FloatMove { opcode, dst, src }
        | Instruction::FloatBinary { opcode, dst, src } => {
            asm.push_str(&format!("  {opcode} {}, {}\n", display(dst), display(src)));
        }
        Instruction::Binary { opcode, dst, src } => {
            asm.push_str(&format!("  {opcode} {}, {}\n", display(dst), display(src)));
        }
        Instruction::Unary { opcode, operand } => {
            asm.push_str(&format!("  {opcode} {}\n", display(operand)));
        }
        Instruction::Compare { opcode, lhs, rhs } => {
            asm.push_str(&format!("  {opcode} {}, {}\n", display(lhs), display(rhs)));
        }
        Instruction::Call { target } => {
            asm.push_str(&format!("  call {}\n", display(target)));
        }
        Instruction::RuntimeCall { target } => {
            asm.push_str(&format!("  call {}\n", display(target)));
        }
        Instruction::Branch { opcode, target } => {
            asm.push_str(&format!("  {opcode} {}\n", display(target)));
        }
        Instruction::Jump { target } => {
            asm.push_str(&format!("  jmp {}\n", display(target)));
        }
        Instruction::Push { src } => {
            asm.push_str(&format!("  push {}\n", display(src)));
        }
        Instruction::Pop { dst } => {
            asm.push_str(&format!("  pop {}\n", display(dst)));
        }
        Instruction::StackAdjust {
            opcode,
            register,
            amount,
        } => {
            asm.push_str(&format!("  {opcode} {register}, {amount}\n"));
        }
        Instruction::Syscall { number } => {
            asm.push_str(&format!("  mov rax, {number}\n  syscall\n"));
        }
        Instruction::SyscallTrap => asm.push_str("  syscall\n"),
        Instruction::PrepareDivision { signed: true } => asm.push_str("  cqo\n"),
        Instruction::PrepareDivision { signed: false } => asm.push_str("  xor rdx, rdx\n"),
        Instruction::Divide { opcode, divisor } => {
            asm.push_str(&format!("  {opcode} {}\n", display(divisor)));
        }
        Instruction::WideMath { opcode, operand } => {
            asm.push_str(&format!("  {opcode} {}\n", display(operand)));
        }
        Instruction::Return => asm.push_str("  ret\n"),
    }
}

fn display(operand: &Operand) -> String {
    match operand {
        Operand::Immediate(value) => value.to_string(),
        Operand::Register(name) | Operand::Address(name) => name.clone(),
        Operand::Memory(address) => display_memory(address),
    }
}

fn display_memory(address: &MemoryAddress) -> String {
    let mut expression = String::new();
    for (index, (operator, term)) in address.terms.iter().enumerate() {
        if index > 0 {
            expression.push_str(match operator {
                AddressOperator::Add => " + ",
                AddressOperator::Subtract => " - ",
            });
        }
        expression.push_str(&match term {
            AddressTerm::Immediate(value) => value.to_string(),
            AddressTerm::Symbol(name) => name.clone(),
            AddressTerm::Register(name) => name.clone(),
            AddressTerm::ScaledRegister { register, scale } => {
                format!("{register} * {scale}")
            }
        });
    }

    match &address.width {
        Some(width) => format!("{width} ptr [{expression}]"),
        None => format!("[{expression}]"),
    }
}
