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
    Assign {
        dst: AssignmentTarget,
        value: AssignmentValue,
    },
    Call {
        target: String,
    },
    Exit {
        code: u8,
    },
    Jmp {
        target: String,
    },
    Let {
        name: String,
        value: BindingValue,
    },
    Print {
        parts: Vec<PrintPart>,
    },
    Pop {
        dst: Operand,
    },
    Push {
        src: Operand,
    },
    Ret,
    Syscall,
}

#[derive(Debug, PartialEq, Clone)]
pub enum AssignmentTarget {
    Operand(Operand),
    RegisterPair { high: String, low: String },
}

#[derive(Debug, PartialEq, Clone)]
pub enum AssignmentValue {
    Operand(Operand),
    Binary {
        op: MathOp,
        lhs: Operand,
        rhs: Operand,
    },
    WideMultiply {
        signed: bool,
        lhs: Operand,
        rhs: Operand,
    },
    WideDivide {
        signed: bool,
        lhs: Operand,
        rhs: Operand,
    },
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MathOp {
    Add,
    Multiply,
    Subtract,
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
