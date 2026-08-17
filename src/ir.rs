use crate::ast::{
    BitwiseUnaryOp, CompareOp, ExprOp, FloatMathOp, IntrinsicOp, MathOp, MemoryWidth,
};

#[derive(Debug, PartialEq, Clone)]
pub struct Program {
    pub entry: String,
    pub data: Vec<DataDeclaration>,
    pub memory: Vec<MemoryDeclaration>,
    pub labels: Vec<Label>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct Label {
    pub name: String,
    pub stack: StackLayout,
    pub instructions: Vec<Instruction>,
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
    Address { target: String },
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
    Array {
        name: String,
        width: MemoryWidth,
        values: Vec<MemoryValue>,
    },
    Repeat {
        name: String,
        width: MemoryWidth,
        count: usize,
        value: MemoryValue,
    },
}

#[derive(Debug, PartialEq, Clone)]
pub enum MemoryValue {
    Integer(i128),
    Address { target: String },
}

#[derive(Debug, PartialEq, Clone)]
pub struct StackLayout {
    pub slots: Vec<StackSlot>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum StackSlot {
    Scalar { name: String, width: MemoryWidth },
    String { name: String },
}

#[derive(Debug, PartialEq, Clone)]
pub enum Instruction {
    Assign {
        dst: Operand,
        value: Value,
    },
    AssignIf {
        dst: Operand,
        value: Value,
        condition: Condition,
    },
    Const {
        name: String,
        value: ConstValue,
    },
    Call {
        target: ControlTarget,
    },
    Exit {
        code: u8,
    },
    Jmp {
        target: ControlTarget,
        condition: Option<Condition>,
    },
    Label {
        name: String,
    },
    Nop,
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
    Runtime(RuntimeOperation),
}

#[derive(Debug, PartialEq, Clone)]
pub enum ConstValue {
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
pub enum Value {
    Operand(Operand),
    Binary {
        op: MathOp,
        lhs: Operand,
        rhs: Operand,
    },
    Expression {
        op: ExprOp,
        lhs: Box<Value>,
        rhs: Box<Value>,
    },
    BitwiseUnary {
        op: BitwiseUnaryOp,
        operand: Operand,
    },
    Condition(Condition),
    FloatBinary {
        width: MemoryWidth,
        op: FloatMathOp,
        lhs: Operand,
        rhs: Operand,
    },
    IntrinsicCall {
        op: IntrinsicOp,
        width: MemoryWidth,
        args: Vec<Operand>,
    },
    StringBytes {
        value: String,
    },
    PlatformReserve {
        len: Operand,
    },
}

#[derive(Debug, PartialEq, Clone)]
pub enum RuntimeOperation {
    Print {
        parts: Vec<PrintPart>,
    },
    Read {
        source: ReadSource,
        dst: Operand,
        len: Operand,
    },
    Release {
        ptr: Operand,
        len: Operand,
    },
}

#[derive(Debug, PartialEq, Clone)]
pub enum PrintPart {
    Binding(String),
    FormattedOperand {
        format: PrintFormat,
        operand: Operand,
    },
    Literal(String),
    Operand(Operand),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PrintFormat {
    Infer,
    SignedDecimal(MemoryWidth),
    UnsignedDecimal(MemoryWidth),
    Hex,
    Binary,
    Pointer,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ReadSource {
    Stdin,
}

#[derive(Debug, PartialEq, Clone)]
pub enum ControlTarget {
    Label(String),
    Operand(Operand),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Operand {
    Immediate(i128),
    FloatLiteral(String),
    Name(String),
    Pointer(String),
    Memory {
        address: Address,
        width: Option<MemoryWidth>,
    },
    AddressOf(Address),
    StringProperty {
        name: String,
        property: StringProperty,
    },
    Converted {
        operand: Box<Operand>,
        conversion: WidthConversion,
    },
    Cast {
        operand: Box<Operand>,
        width: MemoryWidth,
    },
    TargetRegister(String),
}

#[derive(Debug, PartialEq, Clone)]
pub struct Address {
    pub first: AddressTerm,
    pub rest: Vec<(AddressOperator, AddressTerm)>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum AddressTerm {
    Immediate(i128),
    Name(String),
    TargetRegister(String),
    ScaledTargetRegister { register: String, scale: i64 },
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AddressOperator {
    Add,
    Subtract,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum StringProperty {
    Len,
    Ptr,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WidthConversion {
    SignExtend,
    ZeroExtend,
}

#[derive(Debug, PartialEq, Clone)]
pub enum StringInitializer {
    Literal(String),
    Slice { ptr: Operand, len: Operand },
}

#[derive(Debug, PartialEq, Clone)]
pub enum Condition {
    Compare {
        lhs: Operand,
        op: CompareOp,
        rhs: Operand,
    },
    BitwiseAndZero {
        lhs: Operand,
        rhs: Operand,
        op: CompareOp,
    },
}
