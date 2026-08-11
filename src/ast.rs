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
        value: i128,
    },
    FloatScalar {
        name: String,
        width: MemoryWidth,
        value: String,
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
        condition: Option<Condition>,
    },
    Label {
        name: String,
    },
    Const {
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
    Read {
        src: ReadSource,
        dst: Operand,
        len: Operand,
    },
    Ret,
    Stack {
        name: String,
        width: MemoryWidth,
        value: Operand,
    },
    StackString {
        name: String,
        value: StringInitializer,
    },
    Syscall,
}

#[derive(Debug, PartialEq, Clone)]
pub enum ReadSource {
    Stdin,
}

#[derive(Debug, PartialEq, Clone)]
pub enum StringInitializer {
    Literal(String),
    Slice { ptr: Operand, len: Operand },
}

#[derive(Debug, PartialEq, Clone)]
pub struct Condition {
    pub lhs: Operand,
    pub op: CompareOp,
    pub rhs: Operand,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CompareOp {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    SignedLess,
    SignedLessEqual,
    SignedGreater,
    SignedGreaterEqual,
    UnsignedLess,
    UnsignedLessEqual,
    UnsignedGreater,
    UnsignedGreaterEqual,
    FloatEqual(MemoryWidth),
    FloatNotEqual(MemoryWidth),
    FloatLess(MemoryWidth),
    FloatLessEqual(MemoryWidth),
    FloatGreater(MemoryWidth),
    FloatGreaterEqual(MemoryWidth),
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
    FloatBinary {
        width: MemoryWidth,
        op: FloatMathOp,
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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FloatMathOp {
    Add,
    Divide,
    Multiply,
    Subtract,
}

#[derive(Debug, PartialEq, Clone)]
pub enum BindingValue {
    Integer {
        value: i128,
        width: Option<MemoryWidth>,
    },
    Float {
        value: String,
        width: MemoryWidth,
    },
    String(String),
}

#[derive(Debug, PartialEq, Clone)]
pub enum PrintPart {
    Binding(String),
    Literal(String),
    Operand(Operand),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Operand {
    Dereference {
        address: Address,
        width: Option<MemoryWidth>,
    },
    FloatLiteral(String),
    Immediate(i128),
    Register(String),
    Ident(String),
    StringProperty {
        name: String,
        property: StringProperty,
    },
    Pointer(String),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum StringProperty {
    Len,
    Ptr,
}

#[derive(Debug, PartialEq, Eq, std::hash::Hash, Clone, Copy)]
pub enum MemoryWidth {
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
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
    Immediate(i128),
    Register(String),
    ScaledRegister { register: String, scale: i64 },
    Ident(String),
}

#[derive(Debug, PartialEq, Clone)]
pub enum AddressOperator {
    Add,
    Subtract,
}
