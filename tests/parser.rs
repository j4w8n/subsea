use subsea::ast::{
    AssignmentTarget, AssignmentValue, BindingValue, Instruction, MathOp, MemoryDeclaration,
    MemoryWidth, Operand,
};
use subsea::grammar::Token;
use subsea::parser::Parser;

fn parse(tokens: Vec<Token>) -> Result<subsea::ast::Program, String> {
    Parser::new(tokens).parse_program()
}

fn empty_main_prefix() -> Vec<Token> {
    vec![
        Token::Directive(String::from("entry")),
        Token::Ident(String::from("main")),
        Token::Ident(String::from("main")),
        Token::Colon,
        Token::LBrace,
    ]
}

fn finish_label(mut tokens: Vec<Token>) -> Vec<Token> {
    tokens.push(Token::RBrace);
    tokens
}

#[test]
fn parses_integer_binding() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Let,
        Token::Ident(String::from("count")),
        Token::Equals,
        Token::NumberLiteral(String::from("3")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Let {
            name: String::from("count"),
            value: BindingValue::Integer {
                value: 3,
                width: None,
            },
        }
    );
}

#[test]
fn parses_negative_integer_binding() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Let,
        Token::Ident(String::from("count")),
        Token::Equals,
        Token::Minus,
        Token::NumberLiteral(String::from("3")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Let {
            name: String::from("count"),
            value: BindingValue::Integer {
                value: -3,
                width: None,
            },
        }
    );
}

#[test]
fn parses_typed_integer_binding() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Let,
        Token::Ident(String::from("count")),
        Token::Colon,
        Token::Ident(String::from("u8")),
        Token::Equals,
        Token::NumberLiteral(String::from("3")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Let {
            name: String::from("count"),
            value: BindingValue::Integer {
                value: 3,
                width: Some(MemoryWidth::U8),
            },
        }
    );
}

#[test]
fn parses_assignment_math() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Register(String::from("rax")),
        Token::Equals,
        Token::Register(String::from("rbx")),
        Token::Plus,
        Token::NumberLiteral(String::from("3")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Register(String::from("rax"))),
            value: AssignmentValue::Binary {
                op: MathOp::Add,
                lhs: Operand::Register(String::from("rbx")),
                rhs: Operand::Immediate(3),
            },
        }
    );
}

#[test]
fn parses_widened_multiply_assignment() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Register(String::from("rdx")),
        Token::Colon,
        Token::Register(String::from("rax")),
        Token::Equals,
        Token::Register(String::from("rbx")),
        Token::UStar,
        Token::Register(String::from("rcx")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::RegisterPair {
                high: String::from("rdx"),
                low: String::from("rax"),
            },
            value: AssignmentValue::WideMultiply {
                signed: false,
                lhs: Operand::Register(String::from("rbx")),
                rhs: Operand::Register(String::from("rcx")),
            },
        }
    );
}

#[test]
fn parses_widened_divide_assignment() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Register(String::from("rdx")),
        Token::Colon,
        Token::Register(String::from("rax")),
        Token::Equals,
        Token::Register(String::from("rbx")),
        Token::ISlash,
        Token::Register(String::from("rcx")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::RegisterPair {
                high: String::from("rdx"),
                low: String::from("rax"),
            },
            value: AssignmentValue::WideDivide {
                signed: true,
                lhs: Operand::Register(String::from("rbx")),
                rhs: Operand::Register(String::from("rcx")),
            },
        }
    );
}

#[test]
fn parses_call_and_ret() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Call,
        Token::Ident(String::from("helper")),
        Token::Ret,
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![
            Instruction::Call {
                target: String::from("helper"),
            },
            Instruction::Ret,
        ]
    );
}

#[test]
fn parses_push_and_pop() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Push,
        Token::Register(String::from("rax")),
        Token::Pop,
        Token::Register(String::from("rbx")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![
            Instruction::Push {
                src: Operand::Register(String::from("rax")),
            },
            Instruction::Pop {
                dst: Operand::Register(String::from("rbx")),
            },
        ]
    );
}

#[test]
fn rejects_typed_integer_binding_out_of_range() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Let,
        Token::Ident(String::from("count")),
        Token::Colon,
        Token::Ident(String::from("u8")),
        Token::Equals,
        Token::NumberLiteral(String::from("256")),
    ]);

    let error = parse(finish_label(tokens)).unwrap_err();

    assert_eq!(error, "Integer binding value 256 does not fit in u8");
}

#[test]
fn parses_memory_scalar_declaration() {
    let program = parse(vec![
        Token::Directive(String::from("entry")),
        Token::Ident(String::from("main")),
        Token::Mem,
        Token::Ident(String::from("count")),
        Token::Colon,
        Token::Ident(String::from("u16")),
        Token::Equals,
        Token::NumberLiteral(String::from("3")),
        Token::Ident(String::from("main")),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap();

    assert_eq!(
        program.memory[0],
        MemoryDeclaration::Scalar {
            name: String::from("count"),
            width: MemoryWidth::U16,
            value: 3,
        }
    );
}

#[test]
fn parses_memory_buffer_declaration() {
    let program = parse(vec![
        Token::Directive(String::from("entry")),
        Token::Ident(String::from("main")),
        Token::Mem,
        Token::Ident(String::from("buf")),
        Token::Colon,
        Token::Ident(String::from("u8")),
        Token::LParen,
        Token::NumberLiteral(String::from("128")),
        Token::RParen,
        Token::Ident(String::from("main")),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap();

    assert_eq!(
        program.memory[0],
        MemoryDeclaration::Buffer {
            name: String::from("buf"),
            width: MemoryWidth::U8,
            count: 128,
        }
    );
}

#[test]
fn rejects_duplicate_memory_names() {
    let error = parse(vec![
        Token::Directive(String::from("entry")),
        Token::Ident(String::from("main")),
        Token::Mem,
        Token::Ident(String::from("count")),
        Token::Colon,
        Token::Ident(String::from("u16")),
        Token::Equals,
        Token::NumberLiteral(String::from("3")),
        Token::Mem,
        Token::Ident(String::from("count")),
        Token::Colon,
        Token::Ident(String::from("u16")),
        Token::Equals,
        Token::NumberLiteral(String::from("4")),
        Token::Ident(String::from("main")),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap_err();

    assert_eq!(error, "Memory name \"count\" is already defined");
}

#[test]
fn rejects_zero_length_memory_buffer() {
    let error = parse(vec![
        Token::Directive(String::from("entry")),
        Token::Ident(String::from("main")),
        Token::Mem,
        Token::Ident(String::from("buf")),
        Token::Colon,
        Token::Ident(String::from("u8")),
        Token::LParen,
        Token::NumberLiteral(String::from("0")),
        Token::RParen,
        Token::Ident(String::from("main")),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap_err();

    assert_eq!(error, "Buffer count must be greater than 0");
}
