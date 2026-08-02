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
    Copy { src: Operand, dst: Operand },
    Syscall,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Operand {
    Immediate(i64),
    Register(String),
    Ident(String),
    Pointer(String),
}
