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

impl Instruction {
    pub fn operands(&self) -> Vec<&Operand> {
        let mut operands = Vec::new();

        match self {
            Instruction::Assign { dst, value } => {
                if let AssignmentTarget::Operand(operand) = dst {
                    operands.push(operand);
                }

                match value {
                    AssignmentValue::Operand(operand) => operands.push(operand),
                    AssignmentValue::Binary { lhs, rhs, .. }
                    | AssignmentValue::FloatBinary { lhs, rhs, .. }
                    | AssignmentValue::WideMultiply { lhs, rhs, .. }
                    | AssignmentValue::WideDivide { lhs, rhs, .. } => operands.extend([lhs, rhs]),
                }
            }
            Instruction::Jmp {
                condition: Some(condition),
                ..
            } => operands.extend([&condition.lhs, &condition.rhs]),
            Instruction::Print { parts } => {
                operands.extend(parts.iter().filter_map(|part| match part {
                    PrintPart::Operand(operand) => Some(operand),
                    PrintPart::Binding(_) | PrintPart::Literal(_) => None,
                }));
            }
            Instruction::Pop { dst } => operands.push(dst),
            Instruction::Push { src } => operands.push(src),
            Instruction::Read { dst, len, .. } => operands.extend([dst, len]),
            Instruction::Stack { value, .. } => operands.push(value),
            Instruction::StackString {
                value: StringInitializer::Slice { ptr, len },
                ..
            } => operands.extend([ptr, len]),
            _ => {}
        }

        operands
    }
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

impl MemoryWidth {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "f32" => Ok(Self::F32),
            "f64" => Ok(Self::F64),
            "i8" => Ok(Self::I8),
            "i16" => Ok(Self::I16),
            "i32" => Ok(Self::I32),
            "i64" => Ok(Self::I64),
            "u8" => Ok(Self::U8),
            "u16" => Ok(Self::U16),
            "u32" => Ok(Self::U32),
            "u64" => Ok(Self::U64),
            _ => Err(format!(
                "Invalid memory width {name:?}; expected f32, f64, i8, i16, i32, i64, u8, u16, u32, or u64"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
        }
    }

    pub fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    pub fn size(self) -> usize {
        match self {
            Self::F32 | Self::I32 | Self::U32 => 4,
            Self::F64 | Self::I64 | Self::U64 => 8,
            Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
        }
    }

    pub fn directive(self) -> &'static str {
        match self {
            Self::F32 => ".float",
            Self::F64 => ".double",
            Self::I8 | Self::U8 => ".byte",
            Self::I16 | Self::U16 => ".word",
            Self::I32 | Self::U32 => ".long",
            Self::I64 | Self::U64 => ".quad",
        }
    }

    pub fn ptr(self) -> &'static str {
        match self {
            Self::F32 | Self::I32 | Self::U32 => "dword",
            Self::F64 | Self::I64 | Self::U64 => "qword",
            Self::I8 | Self::U8 => "byte",
            Self::I16 | Self::U16 => "word",
        }
    }
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
