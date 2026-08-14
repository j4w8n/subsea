#[derive(Debug, PartialEq, Clone)]
pub struct Program {
    pub entry: String,
    pub imports: Vec<ImportDeclaration>,
    pub exports: Vec<String>,
    pub data: Vec<DataDeclaration>,
    pub memory: Vec<MemoryDeclaration>,
    pub labels: Vec<Label>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct ImportDeclaration {
    pub names: Vec<String>,
    pub path: String,
}

#[derive(Debug, PartialEq, Clone)]
pub struct DataDeclaration {
    pub name: String,
    pub section: String,
    pub align: Option<usize>,
    pub export: bool,
    pub keep: bool,
    pub items: Vec<DataItem>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum DataItem {
    Scalar { width: MemoryWidth, value: i128 },
    Addr { target: String },
    Zero { count: usize },
    Label { name: String },
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
    AssignIf {
        dst: AssignmentTarget,
        value: AssignmentValue,
        condition: ConditionExpr,
    },
    Call {
        target: String,
    },
    Exit {
        code: u8,
    },
    InlineAsm {
        text: String,
    },
    Jmp {
        target: String,
        condition: Option<ConditionExpr>,
    },
    Label {
        name: String,
    },
    Nop,
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
    pub fn visit_operands(&self, mut visit: impl FnMut(&Operand)) {
        match self {
            Instruction::Assign { dst, value } | Instruction::AssignIf { dst, value, .. } => {
                if let AssignmentTarget::Operand(operand) = dst {
                    visit(operand);
                }

                visit_assignment_value_operands(value, &mut visit);

                if let Instruction::AssignIf { condition, .. } = self {
                    condition.visit_operands(&mut visit);
                }
            }
            Instruction::Jmp {
                condition: Some(condition),
                ..
            } => condition.visit_operands(&mut visit),
            Instruction::Print { parts } => {
                for part in parts {
                    if let PrintPart::Operand(operand) = part {
                        visit(operand);
                    }
                }
            }
            Instruction::Pop { dst } => visit(dst),
            Instruction::Push { src } => visit(src),
            Instruction::Read { dst, len, .. } => {
                visit(dst);
                visit(len);
            }
            Instruction::Stack { value, .. } => visit(value),
            Instruction::StackString {
                value: StringInitializer::Slice { ptr, len },
                ..
            } => {
                visit(ptr);
                visit(len);
            }
            _ => {}
        }
    }

    pub fn visit_operands_mut(&mut self, mut visit: impl FnMut(&mut Operand)) {
        match self {
            Instruction::Assign { dst, value } | Instruction::AssignIf { dst, value, .. } => {
                if let AssignmentTarget::Operand(operand) = dst {
                    visit(operand);
                }

                visit_assignment_value_operands_mut(value, &mut visit);

                if let Instruction::AssignIf { condition, .. } = self {
                    condition.visit_operands_mut(&mut visit);
                }
            }
            Instruction::Jmp {
                condition: Some(condition),
                ..
            } => condition.visit_operands_mut(&mut visit),
            Instruction::Print { parts } => {
                for part in parts {
                    if let PrintPart::Operand(operand) = part {
                        visit(operand);
                    }
                }
            }
            Instruction::Pop { dst } => visit(dst),
            Instruction::Push { src } => visit(src),
            Instruction::Read { dst, len, .. } => {
                visit(dst);
                visit(len);
            }
            Instruction::Stack { value, .. } => visit(value),
            Instruction::StackString {
                value: StringInitializer::Slice { ptr, len },
                ..
            } => {
                visit(ptr);
                visit(len);
            }
            _ => {}
        }
    }
}

fn visit_assignment_value_operands(value: &AssignmentValue, visit: &mut impl FnMut(&Operand)) {
    match value {
        AssignmentValue::Operand(operand) => visit(operand),
        AssignmentValue::BitwiseUnary { operand, .. } => visit(operand),
        AssignmentValue::Binary { lhs, rhs, .. }
        | AssignmentValue::FloatBinary { lhs, rhs, .. }
        | AssignmentValue::WideMultiply { lhs, rhs, .. }
        | AssignmentValue::WideDivide { lhs, rhs, .. } => {
            visit(lhs);
            visit(rhs);
        }
        AssignmentValue::Condition(condition) => condition.visit_operands(visit),
    }
}

fn visit_assignment_value_operands_mut(
    value: &mut AssignmentValue,
    visit: &mut impl FnMut(&mut Operand),
) {
    match value {
        AssignmentValue::Operand(operand) => visit(operand),
        AssignmentValue::BitwiseUnary { operand, .. } => visit(operand),
        AssignmentValue::Binary { lhs, rhs, .. }
        | AssignmentValue::FloatBinary { lhs, rhs, .. }
        | AssignmentValue::WideMultiply { lhs, rhs, .. }
        | AssignmentValue::WideDivide { lhs, rhs, .. } => {
            visit(lhs);
            visit(rhs);
        }
        AssignmentValue::Condition(condition) => condition.visit_operands_mut(visit),
    }
}

impl Operand {
    pub fn unconverted(&self) -> &Operand {
        match self {
            Operand::Converted { operand, .. } => operand.unconverted(),
            operand => operand,
        }
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

#[derive(Debug, PartialEq, Clone)]
pub enum ConditionExpr {
    Compare(Condition),
    BitwiseAndZero {
        lhs: Operand,
        rhs: Operand,
        op: CompareOp,
    },
}

impl ConditionExpr {
    pub fn visit_operands(&self, visit: &mut impl FnMut(&Operand)) {
        match self {
            Self::Compare(condition) => {
                visit(&condition.lhs);
                visit(&condition.rhs);
            }
            Self::BitwiseAndZero { lhs, rhs, .. } => {
                visit(lhs);
                visit(rhs);
            }
        }
    }

    pub fn visit_operands_mut(&mut self, visit: &mut impl FnMut(&mut Operand)) {
        match self {
            Self::Compare(condition) => {
                visit(&mut condition.lhs);
                visit(&mut condition.rhs);
            }
            Self::BitwiseAndZero { lhs, rhs, .. } => {
                visit(lhs);
                visit(rhs);
            }
        }
    }
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
    BitwiseUnary {
        op: BitwiseUnaryOp,
        operand: Operand,
    },
    Condition(ConditionExpr),
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
    BitAnd,
    BitOr,
    BitXor,
    Multiply,
    ShiftLeft,
    ShiftRightArithmetic,
    ShiftRightLogical,
    Subtract,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BitwiseUnaryOp {
    Not,
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
    Converted {
        operand: Box<Operand>,
        conversion: WidthConversion,
    },
    Dereference {
        address: Address,
        width: Option<MemoryWidth>,
    },
    AddressOf(Address),
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
pub enum WidthConversion {
    SignExtend,
    ZeroExtend,
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
