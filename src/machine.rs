//! Small target-machine IR used between semantic lowering and assembly text.

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
