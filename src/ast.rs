#[derive(Debug, PartialEq, Clone)]
pub struct Program {
    pub entry: String,
    pub labels: Vec<Label>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct Label {
    pub name: String,
    pub instructions: Vec<Instruction>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Instruction {
    Add { src: Operand, dst: Operand },
    Copy { src: Operand, dst: Operand },
    Div { divisor: Operand },
    Jmp { target: String },
    Mul { src: Operand, dst: Operand },
    Sub { src: Operand, dst: Operand },
    Syscall,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Operand {
    Dereference(Address),
    Immediate(i64),
    Register(String),
    Ident(String),
    Pointer(String),
}

#[derive(Debug, PartialEq, Clone)]
pub struct Address {
    pub first: AddressTerm,
    pub rest: Vec<(AddressOperator, AddressTerm)>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum AddressTerm {
    Immediate(i64),
    Register(String),
    ScaledRegister { register: String, scale: i64 },
    Ident(String),
}

#[derive(Debug, PartialEq, Clone)]
pub enum AddressOperator {
    Add,
    Subtract,
}
