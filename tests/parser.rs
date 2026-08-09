use subsea::ast::{
    AssignmentTarget, AssignmentValue, BindingValue, CompareOp, Condition, FloatMathOp,
    Instruction, MathOp, MemoryDeclaration, MemoryWidth, Operand, PrintPart, ReadSource,
    StringInitializer,
};
use subsea::grammar::Token;
use subsea::parser::{Parser, validate_program_symbols};

fn parse(tokens: Vec<Token>) -> Result<subsea::ast::Program, String> {
    Parser::new(tokens).parse_program()
}

fn empty_main_prefix() -> Vec<Token> {
    vec![
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
        Token::Const,
        Token::Ident(String::from("count")),
        Token::Equals,
        Token::NumberLiteral(String::from("3")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Const {
            name: String::from("count"),
            value: BindingValue::Integer {
                value: 3,
                width: None,
            },
        }
    );
}

#[test]
fn rejects_string_binding_as_operand() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Const,
        Token::Ident(String::from("message")),
        Token::Equals,
        Token::Text(String::from("hi")),
        Token::Register(String::from("rax")),
        Token::Equals,
        Token::Ident(String::from("message")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();
    let error = validate_program_symbols(&program).unwrap_err();

    assert_eq!(
        error,
        "String binding \"message\" in label \"main\" cannot be used as an operand"
    );
}

#[test]
fn parses_negative_integer_binding() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Const,
        Token::Ident(String::from("count")),
        Token::Equals,
        Token::Minus,
        Token::NumberLiteral(String::from("3")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Const {
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
        Token::Const,
        Token::Ident(String::from("count")),
        Token::Colon,
        Token::Ident(String::from("u8")),
        Token::Equals,
        Token::NumberLiteral(String::from("3")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Const {
            name: String::from("count"),
            value: BindingValue::Integer {
                value: 3,
                width: Some(MemoryWidth::U8),
            },
        }
    );
}

#[test]
fn parses_typed_float_binding() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Const,
        Token::Ident(String::from("ratio")),
        Token::Colon,
        Token::Ident(String::from("f64")),
        Token::Equals,
        Token::FloatLiteral(String::from("1.5")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Const {
            name: String::from("ratio"),
            value: BindingValue::Float {
                value: String::from("1.5"),
                width: MemoryWidth::F64,
            },
        }
    );
}

#[test]
fn rejects_untyped_float_binding() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Const,
        Token::Ident(String::from("ratio")),
        Token::Equals,
        Token::FloatLiteral(String::from("1.5")),
    ]);

    let error = parse(finish_label(tokens)).unwrap_err();

    assert_eq!(
        error,
        "Float binding value \"1.5\" requires an explicit f32 or f64 width"
    );
}

#[test]
fn parses_stack_declaration() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Stack,
        Token::Ident(String::from("count")),
        Token::Colon,
        Token::Ident(String::from("u64")),
        Token::Equals,
        Token::NumberLiteral(String::from("8")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Stack {
            name: String::from("count"),
            width: MemoryWidth::U64,
            value: Operand::Immediate(8),
        }]
    );
}

#[test]
fn parses_stack_string_literal() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Stack,
        Token::Ident(String::from("message")),
        Token::Colon,
        Token::Ident(String::from("str")),
        Token::Equals,
        Token::Text(String::from("hello")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::StackString {
            name: String::from("message"),
            value: StringInitializer::Literal(String::from("hello")),
        }]
    );
}

#[test]
fn parses_stack_string_slice() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Stack,
        Token::Ident(String::from("input")),
        Token::Colon,
        Token::Ident(String::from("str")),
        Token::Equals,
        Token::Slice,
        Token::Pointer(String::from("buf")),
        Token::Comma,
        Token::Register(String::from("rax")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::StackString {
            name: String::from("input"),
            value: StringInitializer::Slice {
                ptr: Operand::Pointer(String::from("buf")),
                len: Operand::Register(String::from("rax")),
            },
        }]
    );
}

#[test]
fn parses_read_from_stdin() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Read,
        Token::Stdin,
        Token::Comma,
        Token::Pointer(String::from("buf")),
        Token::Comma,
        Token::NumberLiteral(String::from("1024")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Read {
            src: ReadSource::Stdin,
            dst: Operand::Pointer(String::from("buf")),
            len: Operand::Immediate(1024),
        }]
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
fn parses_print_register() {
    let mut tokens = empty_main_prefix();
    tokens.extend([Token::Print, Token::Register(String::from("rax"))]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Print {
            parts: vec![PrintPart::Operand(Operand::Register(String::from("rax")))],
        }]
    );
}

#[test]
fn parses_nested_label_marker() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::LocalIdent(String::from("loop")),
        Token::Colon,
        Token::Jmp,
        Token::LocalIdent(String::from("loop")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![
            Instruction::Label {
                name: String::from(".L.main.loop"),
            },
            Instruction::Jmp {
                target: String::from(".L.main.loop"),
                condition: None,
            },
        ]
    );
}

#[test]
fn parses_conditional_jump() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Jmp,
        Token::Ident(String::from("done")),
        Token::If,
        Token::Register(String::from("rcx")),
        Token::ULess,
        Token::Register(String::from("rbx")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Jmp {
            target: String::from("done"),
            condition: Some(Condition {
                lhs: Operand::Register(String::from("rcx")),
                op: CompareOp::UnsignedLess,
                rhs: Operand::Register(String::from("rbx")),
            }),
        }]
    );
}

#[test]
fn parses_signed_conditional_jump() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Jmp,
        Token::Ident(String::from("negative")),
        Token::If,
        Token::Register(String::from("rax")),
        Token::ILess,
        Token::NumberLiteral(String::from("0")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Jmp {
            target: String::from("negative"),
            condition: Some(Condition {
                lhs: Operand::Register(String::from("rax")),
                op: CompareOp::SignedLess,
                rhs: Operand::Immediate(0),
            }),
        }]
    );
}

#[test]
fn rejects_conditional_jump_without_signedness() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Jmp,
        Token::Ident(String::from("done")),
        Token::If,
        Token::Register(String::from("rax")),
        Token::Less,
        Token::Register(String::from("rbx")),
    ]);

    let error = parse(finish_label(tokens)).unwrap_err();

    assert_eq!(
        error,
        "Comparison '<' must specify signedness; use i< or u<"
    );
}

#[test]
fn rejects_bare_nested_label() {
    let mut tokens = empty_main_prefix();
    tokens.extend([Token::Ident(String::from("loop")), Token::Colon]);

    let error = parse(finish_label(tokens)).unwrap_err();

    assert_eq!(
        error,
        "Nested label loop: must be local; write .loop: instead"
    );
}

#[test]
fn parses_top_level_bare_label() {
    let program = parse(vec![
        Token::Ident(String::from("main")),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
        Token::Ident(String::from("skip")),
        Token::Colon,
    ])
    .unwrap();

    assert_eq!(
        program.labels[1],
        subsea::ast::Label {
            name: String::from("skip"),
            instructions: Vec::new(),
        }
    );
}

#[test]
fn rejects_top_level_local_label() {
    let error = parse(vec![Token::LocalIdent(String::from("skip")), Token::Colon]).unwrap_err();

    assert_eq!(
        error,
        "Local label .skip cannot be declared at the top level"
    );
}

#[test]
fn rejects_missing_main_label() {
    let error = parse(vec![
        Token::Ident(String::from("helper")),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap_err();

    assert_eq!(error, "Program must define a top-level main label");
}

#[test]
fn rejects_typed_integer_binding_out_of_range() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Const,
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
fn parses_float_memory_scalar_declaration() {
    let program = parse(vec![
        Token::Mem,
        Token::Ident(String::from("ratio")),
        Token::Colon,
        Token::Ident(String::from("f32")),
        Token::Equals,
        Token::FloatLiteral(String::from("1.5")),
        Token::Ident(String::from("main")),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap();

    assert_eq!(
        program.memory[0],
        MemoryDeclaration::FloatScalar {
            name: String::from("ratio"),
            width: MemoryWidth::F32,
            value: String::from("1.5"),
        }
    );
}

#[test]
fn rejects_float_literal_as_operand() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Register(String::from("rax")),
        Token::Equals,
        Token::FloatLiteral(String::from("1.5")),
    ]);

    let error = parse(finish_label(tokens)).unwrap_err();

    assert_eq!(
        error,
        "Float literal 1.5 is only supported in typed const and mem initializers for now"
    );
}

#[test]
fn parses_xmm_float_memory_assignment() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Register(String::from("xmm0")),
        Token::Equals,
        Token::LBracket,
        Token::Ident(String::from("ratio")),
        Token::RBracket,
        Token::Colon,
        Token::Ident(String::from("f64")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Register(String::from("xmm0"))),
            value: AssignmentValue::Operand(Operand::Dereference {
                address: subsea::ast::Address {
                    first: subsea::ast::AddressTerm::Ident(String::from("ratio")),
                    rest: Vec::new(),
                },
                width: Some(MemoryWidth::F64),
            }),
        }
    );
}

#[test]
fn parses_float_arithmetic_assignment() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Register(String::from("xmm0")),
        Token::Equals,
        Token::Register(String::from("xmm1")),
        Token::F64Plus,
        Token::Register(String::from("xmm2")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Register(String::from("xmm0"))),
            value: AssignmentValue::FloatBinary {
                width: MemoryWidth::F64,
                op: FloatMathOp::Add,
                lhs: Operand::Register(String::from("xmm1")),
                rhs: Operand::Register(String::from("xmm2")),
            },
        }
    );
}

#[test]
fn rejects_xmm_register_as_memory_address() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Register(String::from("xmm0")),
        Token::Equals,
        Token::LBracket,
        Token::Register(String::from("xmm1")),
        Token::RBracket,
        Token::Colon,
        Token::Ident(String::from("f64")),
    ]);

    let error = parse(finish_label(tokens)).unwrap_err();

    assert_eq!(
        error,
        "XMM register xmm1 cannot be used as a memory address"
    );
}

#[test]
fn parses_memory_buffer_declaration() {
    let program = parse(vec![
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

#[test]
fn rejects_label_that_conflicts_with_memory_name() {
    let mut program = parse(vec![
        Token::Mem,
        Token::Ident(String::from("count")),
        Token::Colon,
        Token::Ident(String::from("u8")),
        Token::Equals,
        Token::NumberLiteral(String::from("1")),
        Token::Ident(String::from("main")),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
        Token::Ident(String::from("count")),
        Token::Colon,
    ])
    .unwrap();

    let error = subsea::parser::validate_program_symbols(&program).unwrap_err();

    assert_eq!(error, "Label \"count\" conflicts with top-level memory");
    program.labels.pop();
    subsea::parser::validate_program_symbols(&program).unwrap();
}

#[test]
fn rejects_binding_that_conflicts_with_top_level_label() {
    let program = parse(vec![
        Token::Ident(String::from("main")),
        Token::Colon,
        Token::LBrace,
        Token::Const,
        Token::Ident(String::from("count")),
        Token::Equals,
        Token::NumberLiteral(String::from("1")),
        Token::RBrace,
        Token::Ident(String::from("count")),
        Token::Colon,
    ])
    .unwrap();

    let error = subsea::parser::validate_program_symbols(&program).unwrap_err();

    assert_eq!(
        error,
        "Name \"count\" in label \"main\" conflicts with top-level label"
    );
}

#[test]
fn rejects_stack_variable_that_conflicts_with_top_level_label() {
    let program = parse(vec![
        Token::Ident(String::from("main")),
        Token::Colon,
        Token::LBrace,
        Token::Stack,
        Token::Ident(String::from("count")),
        Token::Colon,
        Token::Ident(String::from("u64")),
        Token::Equals,
        Token::NumberLiteral(String::from("1")),
        Token::RBrace,
        Token::Ident(String::from("count")),
        Token::Colon,
    ])
    .unwrap();

    let error = subsea::parser::validate_program_symbols(&program).unwrap_err();

    assert_eq!(
        error,
        "Name \"count\" in label \"main\" conflicts with top-level label"
    );
}

#[test]
fn parses_max_u64_binding() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Const,
        Token::Ident(String::from("max")),
        Token::Colon,
        Token::Ident(String::from("u64")),
        Token::Equals,
        Token::NumberLiteral(String::from("18446744073709551615")),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Const {
            name: String::from("max"),
            value: BindingValue::Integer {
                value: 18446744073709551615,
                width: Some(MemoryWidth::U64),
            },
        }
    );
}
