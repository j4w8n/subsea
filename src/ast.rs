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
    Idiv { divisor: Operand },
    Imul { src: Operand, dst: Operand },
    Jmp { target: String },
    LetString { name: String, value: String },
    Print { target: PrintTarget },
    Sub { src: Operand, dst: Operand },
    Syscall,
    Udiv { divisor: Operand },
    Umul { src: Operand, dst: Operand },
}

#[derive(Debug, PartialEq, Clone)]
pub enum PrintTarget {
    Binding(String),
    Literal(String),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Operand {
    Dereference {
        address: Address,
        width: Option<MemoryWidth>,
    },
    Immediate(i64),
    Register(String),
    Ident(String),
    Pointer(String),
}

#[derive(Debug, PartialEq, Clone)]
pub enum MemoryWidth {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
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
