#[derive(Debug, PartialEq, Clone)]
pub struct Program {
    pub entry: String,
    pub memory: Vec<MemoryDeclaration>,
    pub labels: Vec<Label>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum MemoryDeclaration {
    Scalar {
        name: String,
        width: MemoryWidth,
        value: i64,
    },
    Buffer {
        name: String,
        width: MemoryWidth,
        count: usize,
    },
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
    Exit { code: u8 },
    Idiv { divisor: Operand },
    Imul { src: Operand, dst: Operand },
    Jmp { target: String },
    Let { name: String, value: BindingValue },
    Print { parts: Vec<PrintPart> },
    Sub { src: Operand, dst: Operand },
    Syscall,
    Udiv { divisor: Operand },
    Umul { src: Operand, dst: Operand },
}

#[derive(Debug, PartialEq, Clone)]
pub enum BindingValue {
    Integer {
        value: i64,
        width: Option<MemoryWidth>,
    },
    String(String),
}

#[derive(Debug, PartialEq, Clone)]
pub enum PrintPart {
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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
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
