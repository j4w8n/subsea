use subsea::ast::{
    AssignmentTarget, AssignmentValue, BindingValue, CompareOp, Condition, ConditionExpr,
    FloatMathOp, Instruction, MathOp, MemoryDeclaration, MemoryWidth, Operand, PrintPart,
    ReadSource, StringInitializer, StringProperty, WidthConversion,
};
use subsea::grammar::Token;
use subsea::parser::{Parser, validate_program_symbols};

fn parse(tokens: Vec<Token>) -> Result<subsea::ast::Program, String> {
    Parser::new(tokens).parse_program()
}

fn s(value: &str) -> String {
    value.to_string()
}

fn cmp(condition: Condition) -> ConditionExpr {
    ConditionExpr::Compare(condition)
}

fn tid(value: &str) -> Token {
    Token::Ident(s(value))
}

fn tlocal(value: &str) -> Token {
    Token::LocalIdent(s(value))
}

fn tnum(value: &str) -> Token {
    Token::NumberLiteral(s(value))
}

fn tfloat(value: &str) -> Token {
    Token::FloatLiteral(s(value))
}

fn text(value: &str) -> Token {
    Token::Text(s(value))
}

fn tptr(value: &str) -> Token {
    Token::Pointer(s(value))
}

fn treg(value: &str) -> Token {
    Token::Register(s(value))
}

fn ptr(value: &str) -> Operand {
    Operand::Pointer(s(value))
}

fn reg(value: &str) -> Operand {
    Operand::Register(s(value))
}

fn empty_main_prefix() -> Vec<Token> {
    vec![tid("main"), Token::Colon, Token::LBrace]
}

fn finish_label(mut tokens: Vec<Token>) -> Vec<Token> {
    tokens.push(Token::RBrace);
    tokens
}

#[test]
fn parses_integer_binding() {
    let mut tokens = empty_main_prefix();
    tokens.extend([Token::Const, tid("count"), Token::Equals, tnum("3")]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Const {
            name: s("count"),
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
        tid("message"),
        Token::Equals,
        text("hi"),
        treg("rax"),
        Token::Equals,
        tid("message"),
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
        tid("count"),
        Token::Equals,
        Token::Minus,
        tnum("3"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Const {
            name: s("count"),
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
        tid("count"),
        Token::Colon,
        tid("u8"),
        Token::Equals,
        tnum("3"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Const {
            name: s("count"),
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
        tid("ratio"),
        Token::Colon,
        tid("f64"),
        Token::Equals,
        tfloat("1.5"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Const {
            name: s("ratio"),
            value: BindingValue::Float {
                value: s("1.5"),
                width: MemoryWidth::F64,
            },
        }
    );
}

#[test]
fn rejects_untyped_float_binding() {
    let mut tokens = empty_main_prefix();
    tokens.extend([Token::Const, tid("ratio"), Token::Equals, tfloat("1.5")]);

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
        tid("count"),
        Token::Colon,
        tid("u64"),
        Token::Equals,
        tnum("8"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Stack {
            name: s("count"),
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
        tid("message"),
        Token::Colon,
        tid("str"),
        Token::Equals,
        text("hello"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::StackString {
            name: s("message"),
            value: StringInitializer::Literal(s("hello")),
        }]
    );
}

#[test]
fn parses_stack_string_slice() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Stack,
        tid("input"),
        Token::Colon,
        tid("str"),
        Token::Equals,
        Token::Slice,
        tptr("buf"),
        Token::Comma,
        treg("rax"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::StackString {
            name: s("input"),
            value: StringInitializer::Slice {
                ptr: ptr("buf"),
                len: reg("rax"),
            },
        }]
    );
}

#[test]
fn parses_stack_string_properties_as_operands() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        tid("message"),
        tlocal("ptr"),
        treg("rbx"),
        Token::Equals,
        tid("message"),
        tlocal("len"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![
            Instruction::Assign {
                dst: AssignmentTarget::Operand(reg("rax")),
                value: AssignmentValue::Operand(Operand::StringProperty {
                    name: s("message"),
                    property: StringProperty::Ptr,
                }),
            },
            Instruction::Assign {
                dst: AssignmentTarget::Operand(reg("rbx")),
                value: AssignmentValue::Operand(Operand::StringProperty {
                    name: s("message"),
                    property: StringProperty::Len,
                }),
            },
        ]
    );
}

#[test]
fn parses_stack_string_property_print_as_operand() {
    let mut tokens = empty_main_prefix();
    tokens.extend([Token::Print, tid("message"), tlocal("len")]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Print {
            parts: vec![PrintPart::Operand(Operand::StringProperty {
                name: s("message"),
                property: StringProperty::Len,
            })],
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
        tptr("buf"),
        Token::Comma,
        tnum("1024"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Read {
            src: ReadSource::Stdin,
            dst: ptr("buf"),
            len: Operand::Immediate(1024),
        }]
    );
}

#[test]
fn parses_assignment_math() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        treg("rbx"),
        Token::Plus,
        tnum("3"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Binary {
                op: MathOp::Add,
                lhs: reg("rbx"),
                rhs: Operand::Immediate(3),
            },
        }
    );
}

#[test]
fn parses_bitwise_assignment() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        treg("rbx"),
        Token::Ampersand,
        treg("rcx"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Binary {
                op: MathOp::BitAnd,
                lhs: reg("rbx"),
                rhs: reg("rcx"),
            },
        }
    );
}

#[test]
fn parses_unary_bitwise_not_assignment() {
    let mut tokens = empty_main_prefix();
    tokens.extend([treg("rax"), Token::Equals, Token::Tilde, treg("rbx")]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::BitwiseUnary {
                op: subsea::ast::BitwiseUnaryOp::Not,
                operand: reg("rbx"),
            },
        }
    );
}

#[test]
fn parses_arithmetic_shift_right_assignment() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        treg("rbx"),
        Token::IShiftRight,
        tnum("3"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Binary {
                op: MathOp::ShiftRightArithmetic,
                lhs: reg("rbx"),
                rhs: Operand::Immediate(3),
            },
        }
    );
}

#[test]
fn parses_width_converted_operand() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        treg("al"),
        Token::DoubleColon,
        tid("zx"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Operand(Operand::Converted {
                operand: Box::new(reg("al")),
                conversion: WidthConversion::ZeroExtend,
            }),
        }
    );
}

#[test]
fn parses_indexed_memory_operand() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        tid("values"),
        Token::LBracket,
        treg("r8"),
        Token::Star,
        tnum("8"),
        Token::RBracket,
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Operand(Operand::Dereference {
                address: subsea::ast::Address {
                    first: subsea::ast::AddressTerm::Ident(s("values")),
                    rest: vec![(
                        subsea::ast::AddressOperator::Add,
                        subsea::ast::AddressTerm::ScaledRegister {
                            register: s("r8"),
                            scale: 8,
                        },
                    )],
                },
                width: None,
            }),
        }
    );
}

#[test]
fn parses_indexed_memory_assignment_target() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        tid("values"),
        Token::LBracket,
        treg("r8"),
        Token::Star,
        tnum("8"),
        Token::RBracket,
        Token::Equals,
        treg("rax"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert!(matches!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Dereference { .. }),
            ..
        }
    ));
}

#[test]
fn parses_address_of_indexed_memory() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rsi"),
        Token::Equals,
        Token::Ampersand,
        tid("buf"),
        Token::LBracket,
        treg("rax"),
        Token::RBracket,
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert!(matches!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            value: AssignmentValue::Operand(Operand::AddressOf(_)),
            ..
        }
    ));
}

#[test]
fn rejects_unknown_width_conversion() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        treg("al"),
        Token::DoubleColon,
        tid("wide"),
    ]);

    let error = parse(finish_label(tokens)).unwrap_err();

    assert_eq!(
        error,
        "Unknown width conversion ::wide; expected ::zx or ::sx"
    );
}

#[test]
fn parses_boolean_comparison_assignment() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        treg("rdi"),
        Token::ILess,
        treg("rsi"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Condition(cmp(Condition {
                lhs: reg("rdi"),
                op: CompareOp::SignedLess,
                rhs: reg("rsi"),
            })),
        }
    );
}

#[test]
fn parses_conditional_assignment() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        treg("rbx"),
        Token::If,
        treg("rcx"),
        Token::EqualsEquals,
        tnum("0"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::AssignIf {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Operand(reg("rbx")),
            condition: cmp(Condition {
                lhs: reg("rcx"),
                op: CompareOp::Equal,
                rhs: Operand::Immediate(0),
            }),
        }
    );
}

#[test]
fn parses_bitwise_and_zero_condition() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Jmp,
        tlocal("set"),
        Token::If,
        treg("rax"),
        Token::Ampersand,
        tnum("8"),
        Token::NotEquals,
        tnum("0"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Jmp {
            target: s(".L.main.set"),
            condition: Some(ConditionExpr::BitwiseAndZero {
                lhs: reg("rax"),
                rhs: Operand::Immediate(8),
                op: CompareOp::NotEqual,
            }),
        }
    );
}

#[test]
fn parses_widened_multiply_assignment() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rdx"),
        Token::Colon,
        treg("rax"),
        Token::Equals,
        treg("rbx"),
        Token::UStar,
        treg("rcx"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::RegisterPair {
                high: s("rdx"),
                low: s("rax"),
            },
            value: AssignmentValue::WideMultiply {
                signed: false,
                lhs: reg("rbx"),
                rhs: reg("rcx"),
            },
        }
    );
}

#[test]
fn parses_widened_divide_assignment() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rdx"),
        Token::Colon,
        treg("rax"),
        Token::Equals,
        treg("rbx"),
        Token::ISlash,
        treg("rcx"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::RegisterPair {
                high: s("rdx"),
                low: s("rax"),
            },
            value: AssignmentValue::WideDivide {
                signed: true,
                lhs: reg("rbx"),
                rhs: reg("rcx"),
            },
        }
    );
}

#[test]
fn parses_call_and_ret() {
    let mut tokens = empty_main_prefix();
    tokens.extend([Token::Call, tid("helper"), Token::Ret]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![
            Instruction::Call {
                target: s("helper"),
            },
            Instruction::Ret,
        ]
    );
}

#[test]
fn parses_push_and_pop() {
    let mut tokens = empty_main_prefix();
    tokens.extend([Token::Push, treg("rax"), Token::Pop, treg("rbx")]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![
            Instruction::Push { src: reg("rax") },
            Instruction::Pop { dst: reg("rbx") },
        ]
    );
}

#[test]
fn parses_print_register() {
    let mut tokens = empty_main_prefix();
    tokens.extend([Token::Print, treg("rax")]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Print {
            parts: vec![PrintPart::Operand(reg("rax"))],
        }]
    );
}

#[test]
fn parses_nested_label_marker() {
    let mut tokens = empty_main_prefix();
    tokens.extend([tlocal("loop"), Token::Colon, Token::Jmp, tlocal("loop")]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![
            Instruction::Label {
                name: s(".L.main.loop"),
            },
            Instruction::Jmp {
                target: s(".L.main.loop"),
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
        tid("done"),
        Token::If,
        treg("rcx"),
        Token::ULess,
        treg("rbx"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Jmp {
            target: s("done"),
            condition: Some(cmp(Condition {
                lhs: reg("rcx"),
                op: CompareOp::UnsignedLess,
                rhs: reg("rbx"),
            })),
        }]
    );
}

#[test]
fn parses_signed_conditional_jump() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Jmp,
        tid("negative"),
        Token::If,
        treg("rax"),
        Token::ILess,
        tnum("0"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Jmp {
            target: s("negative"),
            condition: Some(cmp(Condition {
                lhs: reg("rax"),
                op: CompareOp::SignedLess,
                rhs: Operand::Immediate(0),
            })),
        }]
    );
}

#[test]
fn parses_conditional_jump_without_resolved_signedness() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Jmp,
        tid("done"),
        Token::If,
        treg("rax"),
        Token::Less,
        treg("rbx"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Jmp {
            target: s("done"),
            condition: Some(cmp(Condition {
                lhs: reg("rax"),
                op: CompareOp::Less,
                rhs: reg("rbx"),
            })),
        }]
    );
}

#[test]
fn rejects_bare_nested_label() {
    let mut tokens = empty_main_prefix();
    tokens.extend([tid("loop"), Token::Colon]);

    let error = parse(finish_label(tokens)).unwrap_err();

    assert_eq!(
        error,
        "Nested label loop: must be local; write .loop: instead"
    );
}

#[test]
fn parses_top_level_bare_label() {
    let program = parse(vec![
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
        tid("skip"),
        Token::Colon,
    ])
    .unwrap();

    assert_eq!(
        program.labels[1],
        subsea::ast::Label {
            name: s("skip"),
            instructions: Vec::new(),
        }
    );
}

#[test]
fn rejects_top_level_local_label() {
    let error = parse(vec![tlocal("skip"), Token::Colon]).unwrap_err();

    assert_eq!(
        error,
        "Local label .skip cannot be declared at the top level"
    );
}

#[test]
fn rejects_missing_main_label() {
    let error = parse(vec![
        tid("helper"),
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
        tid("count"),
        Token::Colon,
        tid("u8"),
        Token::Equals,
        tnum("256"),
    ]);

    let error = parse(finish_label(tokens)).unwrap_err();

    assert_eq!(error, "Integer binding value 256 does not fit in u8");
}

#[test]
fn parses_memory_scalar_declaration() {
    let program = parse(vec![
        Token::Mem,
        tid("count"),
        Token::Colon,
        tid("u16"),
        Token::Equals,
        tnum("3"),
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap();

    assert_eq!(
        program.memory[0],
        MemoryDeclaration::Scalar {
            name: s("count"),
            width: MemoryWidth::U16,
            value: 3,
        }
    );
}

#[test]
fn parses_float_memory_scalar_declaration() {
    let program = parse(vec![
        Token::Mem,
        tid("ratio"),
        Token::Colon,
        tid("f32"),
        Token::Equals,
        tfloat("1.5"),
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap();

    assert_eq!(
        program.memory[0],
        MemoryDeclaration::FloatScalar {
            name: s("ratio"),
            width: MemoryWidth::F32,
            value: s("1.5"),
        }
    );
}

#[test]
fn parses_float_literal_as_operand() {
    let mut tokens = empty_main_prefix();
    tokens.extend([treg("rax"), Token::Equals, tfloat("1.5")]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Operand(Operand::FloatLiteral(s("1.5"))),
        }
    );
}

#[test]
fn parses_stack_float_declaration() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Stack,
        tid("ratio"),
        Token::Colon,
        tid("f64"),
        Token::Equals,
        tfloat("1.5"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Stack {
            name: s("ratio"),
            width: MemoryWidth::F64,
            value: Operand::FloatLiteral(s("1.5")),
        }]
    );
}

#[test]
fn parses_float_conditional_jump() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Jmp,
        tid("done"),
        Token::If,
        treg("xmm0"),
        Token::F64Less,
        tfloat("1.5"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Jmp {
            target: s("done"),
            condition: Some(cmp(Condition {
                lhs: reg("xmm0"),
                op: CompareOp::FloatLess(MemoryWidth::F64),
                rhs: Operand::FloatLiteral(s("1.5")),
            })),
        }]
    );
}

#[test]
fn parses_xmm_float_memory_assignment() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("xmm0"),
        Token::Equals,
        Token::LBracket,
        tid("ratio"),
        Token::RBracket,
        Token::Colon,
        tid("f64"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm0")),
            value: AssignmentValue::Operand(Operand::Dereference {
                address: subsea::ast::Address {
                    first: subsea::ast::AddressTerm::Ident(s("ratio")),
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
        treg("xmm0"),
        Token::Equals,
        treg("xmm1"),
        Token::F64Plus,
        treg("xmm2"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm0")),
            value: AssignmentValue::FloatBinary {
                width: MemoryWidth::F64,
                op: FloatMathOp::Add,
                lhs: reg("xmm1"),
                rhs: reg("xmm2"),
            },
        }
    );
}

#[test]
fn rejects_xmm_register_as_memory_address() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("xmm0"),
        Token::Equals,
        Token::LBracket,
        treg("xmm1"),
        Token::RBracket,
        Token::Colon,
        tid("f64"),
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
        tid("buf"),
        Token::Colon,
        tid("u8"),
        Token::LParen,
        tnum("128"),
        Token::RParen,
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap();

    assert_eq!(
        program.memory[0],
        MemoryDeclaration::Buffer {
            name: s("buf"),
            width: MemoryWidth::U8,
            count: 128,
        }
    );
}

#[test]
fn rejects_duplicate_memory_names() {
    let error = parse(vec![
        Token::Mem,
        tid("count"),
        Token::Colon,
        tid("u16"),
        Token::Equals,
        tnum("3"),
        Token::Mem,
        tid("count"),
        Token::Colon,
        tid("u16"),
        Token::Equals,
        tnum("4"),
        tid("main"),
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
        tid("buf"),
        Token::Colon,
        tid("u8"),
        Token::LParen,
        tnum("0"),
        Token::RParen,
        tid("main"),
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
        tid("count"),
        Token::Colon,
        tid("u8"),
        Token::Equals,
        tnum("1"),
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
        tid("count"),
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
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::Const,
        tid("count"),
        Token::Equals,
        tnum("1"),
        Token::RBrace,
        tid("count"),
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
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::Stack,
        tid("count"),
        Token::Colon,
        tid("u64"),
        Token::Equals,
        tnum("1"),
        Token::RBrace,
        tid("count"),
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
        tid("max"),
        Token::Colon,
        tid("u64"),
        Token::Equals,
        tnum("18446744073709551615"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Const {
            name: s("max"),
            value: BindingValue::Integer {
                value: 18446744073709551615,
                width: Some(MemoryWidth::U64),
            },
        }
    );
}
