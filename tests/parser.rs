use subsea::ast::{
    AssignmentTarget, AssignmentValue, BindingValue, CompareOp, Condition, ConditionExpr,
    DataDeclaration, DataItem, ExprOp, Expression, FloatMathOp, Instruction, IntrinsicOp, MathOp,
    MemoryDeclaration, MemoryValue, MemoryWidth, Operand, PrintPart, ReadSource, StringInitializer,
    StringProperty, WidthConversion,
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

fn linux(operation: &str) -> [Token; 2] {
    [tid("linux"), tlocal(operation)]
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
fn parses_import_declaration() {
    let program = parse(vec![
        Token::Import,
        tid("debug_write"),
        Token::Comma,
        tid("panic_halt"),
        Token::From,
        text("debug.ss"),
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap();

    assert_eq!(program.imports.len(), 1);
    assert_eq!(
        program.imports[0].names,
        vec![s("debug_write"), s("panic_halt")]
    );
    assert_eq!(program.imports[0].path, s("debug.ss"));
}

#[test]
fn parses_exported_function() {
    let program = parse(vec![
        Token::Export,
        tid("debug_write"),
        Token::Colon,
        Token::LBrace,
        Token::Ret,
        Token::RBrace,
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap();

    assert_eq!(program.exports, vec![s("debug_write")]);
    assert_eq!(program.labels[0].name, s("debug_write"));
}

#[test]
fn rejects_exported_bare_label() {
    let error = parse(vec![Token::Export, tid("debug_write"), Token::Colon]).unwrap_err();

    assert_eq!(error, "Exported function \"debug_write\" must have a block");
}

#[test]
fn parses_data_block() {
    let program = parse(vec![
        Token::Data,
        tid("request"),
        Token::Section,
        text(".requests"),
        Token::Align,
        tnum("8"),
        Token::Export,
        Token::Keep,
        Token::LBrace,
        tid("u64"),
        tnum("1"),
        Token::Addr,
        tid("response"),
        Token::Zero,
        tnum("16"),
        tid("response"),
        Token::Colon,
        tid("u64"),
        tnum("0"),
        Token::RBrace,
        tid("main"),
        Token::Colon,
        Token::LBrace,
        tid("linux"),
        tlocal("exit"),
        tnum("0"),
        Token::RBrace,
    ])
    .unwrap();

    assert_eq!(
        program.data[0],
        DataDeclaration {
            name: s("request"),
            section: s(".requests"),
            align: Some(8),
            export: true,
            keep: true,
            items: vec![
                DataItem::Scalar {
                    width: MemoryWidth::U64,
                    value: 1,
                },
                DataItem::Addr {
                    target: s("response"),
                },
                DataItem::Zero { count: 16 },
                DataItem::Label {
                    name: s("response"),
                },
                DataItem::Scalar {
                    width: MemoryWidth::U64,
                    value: 0,
                },
            ],
        }
    );
}

#[test]
fn rejects_non_power_of_two_data_alignment() {
    let error = parse(vec![
        Token::Data,
        tid("request"),
        Token::Section,
        text(".requests"),
        Token::Align,
        tnum("3"),
        Token::LBrace,
        Token::RBrace,
        tid("main"),
        Token::Colon,
        Token::LBrace,
        tid("linux"),
        tlocal("exit"),
        tnum("0"),
        Token::RBrace,
    ])
    .unwrap_err();

    assert_eq!(
        error,
        "Data block \"request\" alignment must be a non-zero power of two"
    );
}

#[test]
fn rejects_unknown_data_addr_target() {
    let program = parse(vec![
        Token::Data,
        tid("request"),
        Token::Section,
        text(".requests"),
        Token::LBrace,
        Token::Addr,
        tid("missing"),
        Token::RBrace,
        tid("main"),
        Token::Colon,
        Token::LBrace,
        tid("linux"),
        tlocal("exit"),
        tnum("0"),
        Token::RBrace,
    ])
    .unwrap();
    let error = validate_program_symbols(&program).unwrap_err();

    assert_eq!(
        error,
        "Unknown address target \"missing\" in data block \"request\""
    );
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
fn rejects_integer_binding_string_property() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        Token::Const,
        tid("count"),
        Token::Equals,
        tnum("3"),
        treg("rax"),
        Token::Equals,
        tid("count"),
        tlocal("len"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();
    let error = validate_program_symbols(&program).unwrap_err();

    assert_eq!(error, "Binding \"count\" in label \"main\" is not a string");
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
        Token::LParen,
        tptr("buf"),
        Token::Comma,
        treg("rax"),
        Token::RParen,
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
    tokens.extend(linux("print"));
    tokens.extend([tid("message"), tlocal("len")]);

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
    tokens.extend(linux("read"));
    tokens.extend([
        Token::LParen,
        Token::Stdin,
        Token::Comma,
        tptr("buf"),
        Token::Comma,
        tnum("1024"),
        Token::RParen,
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
fn parses_arithmetic_expression_with_precedence() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        tnum("2"),
        Token::Plus,
        tnum("3"),
        Token::Star,
        tnum("4"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Expression(Expression::Binary {
                op: ExprOp::Math(MathOp::Add),
                lhs: Box::new(Expression::Operand(Operand::Immediate(2))),
                rhs: Box::new(Expression::Binary {
                    op: ExprOp::Math(MathOp::Multiply),
                    lhs: Box::new(Expression::Operand(Operand::Immediate(3))),
                    rhs: Box::new(Expression::Operand(Operand::Immediate(4))),
                }),
            }),
        }
    );
}

#[test]
fn parses_parenthesized_arithmetic_expression() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        Token::LParen,
        tnum("2"),
        Token::Plus,
        tnum("3"),
        Token::RParen,
        Token::Star,
        tnum("4"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert!(matches!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            value: AssignmentValue::Expression(_),
            ..
        }
    ));
}

#[test]
fn rejects_plain_modulo_without_signedness() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        tnum("5"),
        Token::Percent,
        tnum("2"),
    ]);

    let error = parse(finish_label(tokens)).unwrap_err();

    assert_eq!(error, "Modulo must specify signedness; use i% or u%");
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
fn parses_cast_operand() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("xmm0"),
        Token::Equals,
        treg("rax"),
        Token::DoubleColon,
        tid("f64"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions[0],
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm0")),
            value: AssignmentValue::Operand(Operand::Cast {
                operand: Box::new(reg("rax")),
                width: MemoryWidth::F64,
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
fn parses_address_of_raw_address_expression() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        Token::Ampersand,
        Token::LBracket,
        treg("rbx"),
        Token::Plus,
        treg("rcx"),
        Token::Star,
        tnum("4"),
        Token::Plus,
        tnum("8"),
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
        "Unknown conversion ::wide; expected ::zx, ::sx, or a memory width"
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
fn parses_nop() {
    let mut tokens = empty_main_prefix();
    tokens.extend([Token::Nop]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(program.labels[0].instructions, vec![Instruction::Nop]);
}

#[test]
fn parses_hlt() {
    let mut tokens = empty_main_prefix();
    tokens.extend([Token::X86, text("hlt")]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::InlineAsm { text: s("hlt") }]
    );
}

#[test]
fn parses_port_io() {
    let mut tokens = empty_main_prefix();
    tokens.extend([Token::X86, text("out 0x80, al")]);
    tokens.extend([Token::X86, text("in al, dx")]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![
            Instruction::InlineAsm {
                text: s("out 0x80, al"),
            },
            Instruction::InlineAsm {
                text: s("in al, dx"),
            },
        ]
    );
}

#[test]
fn rejects_multiline_x86_assembly() {
    let mut tokens = empty_main_prefix();
    tokens.extend([Token::X86, text("hlt\nnop")]);

    let error = parse(finish_label(tokens)).unwrap_err();

    assert_eq!(error, "x86 assembly must be a single line");
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
    tokens.extend(linux("print"));
    tokens.extend([treg("rax")]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![Instruction::Print {
            parts: vec![PrintPart::Operand(reg("rax"))],
        }]
    );
}

#[test]
fn rejects_unqualified_target_specific_instructions_with_suggestions() {
    for (token, message) in [
        (
            Token::Print,
            "Unknown instruction \"print\"; did you mean linux.print?",
        ),
        (
            Token::Read,
            "Unknown instruction \"read\"; did you mean linux.read?",
        ),
        (
            Token::Exit,
            "Unknown instruction \"exit\"; did you mean linux.exit?",
        ),
        (
            Token::Syscall,
            "Unknown instruction \"syscall\"; did you mean linux.syscall?",
        ),
        (
            Token::Halt,
            "Unknown instruction \"hlt\"; use x86 \"hlt\" for raw x86 assembly",
        ),
        (
            Token::In,
            "Unknown instruction \"in\"; use x86 \"in\" for raw x86 assembly",
        ),
        (
            Token::Out,
            "Unknown instruction \"out\"; use x86 \"out\" for raw x86 assembly",
        ),
    ] {
        let mut tokens = empty_main_prefix();
        tokens.push(token);

        assert_eq!(parse(finish_label(tokens)).unwrap_err(), message);
    }
}

#[test]
fn rejects_unknown_namespaced_instruction() {
    let mut tokens = empty_main_prefix();
    tokens.extend([Token::X86, tlocal("print")]);

    assert_eq!(
        parse(finish_label(tokens)).unwrap_err(),
        "Expected string literal after x86, found LocalIdent(\"print\")"
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
fn parses_memory_array_declaration() {
    let program = parse(vec![
        Token::Mem,
        tid("values"),
        Token::Colon,
        tid("u16"),
        Token::Equals,
        Token::LBracket,
        tnum("1"),
        Token::Comma,
        tnum("2"),
        Token::Comma,
        tnum("3"),
        Token::RBracket,
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap();

    assert_eq!(
        program.memory[0],
        MemoryDeclaration::Array {
            name: s("values"),
            width: MemoryWidth::U16,
            values: vec![
                MemoryValue::Integer(1),
                MemoryValue::Integer(2),
                MemoryValue::Integer(3),
            ],
        }
    );
}

#[test]
fn parses_memory_string_bytes_declaration() {
    let program = parse(vec![
        Token::Mem,
        tid("greeting"),
        Token::Colon,
        tid("u8"),
        Token::Equals,
        text("hi\n"),
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap();

    assert_eq!(
        program.memory[0],
        MemoryDeclaration::Array {
            name: s("greeting"),
            width: MemoryWidth::U8,
            values: vec![
                MemoryValue::Integer(b'h' as i128),
                MemoryValue::Integer(b'i' as i128),
                MemoryValue::Integer(b'\n' as i128),
            ],
        }
    );
}

#[test]
fn parses_memory_repeat_declaration() {
    let program = parse(vec![
        Token::Mem,
        tid("fill"),
        Token::Colon,
        tid("u8"),
        Token::Equals,
        Token::Repeat,
        Token::LParen,
        tnum("4"),
        Token::Comma,
        tnum("255"),
        Token::RParen,
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap();

    assert_eq!(
        program.memory[0],
        MemoryDeclaration::Repeat {
            name: s("fill"),
            width: MemoryWidth::U8,
            count: 4,
            value: MemoryValue::Integer(255),
        }
    );
}

#[test]
fn parses_memory_pointer_address_array() {
    let program = parse(vec![
        Token::Mem,
        tid("table"),
        Token::Colon,
        tid("ptr"),
        Token::Equals,
        Token::LBracket,
        Token::Addr,
        tid("main"),
        Token::Comma,
        Token::Addr,
        tid("helper"),
        Token::RBracket,
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
        tid("helper"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap();

    assert_eq!(
        program.memory[0],
        MemoryDeclaration::Array {
            name: s("table"),
            width: MemoryWidth::Ptr,
            values: vec![
                MemoryValue::Addr { target: s("main") },
                MemoryValue::Addr {
                    target: s("helper")
                },
            ],
        }
    );
}

#[test]
fn rejects_string_initializer_for_non_u8_memory() {
    let error = parse(vec![
        Token::Mem,
        tid("greeting"),
        Token::Colon,
        tid("u16"),
        Token::Equals,
        text("hi"),
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap_err();

    assert_eq!(error, "String memory initializers require u8 memory width");
}

#[test]
fn rejects_integer_initializer_for_ptr_memory() {
    let error = parse(vec![
        Token::Mem,
        tid("callback"),
        Token::Colon,
        tid("ptr"),
        Token::Equals,
        tnum("0"),
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap_err();

    assert_eq!(error, "ptr memory initializers require addr <symbol>");
}

#[test]
fn rejects_unknown_memory_address_target() {
    let program = parse(vec![
        Token::Mem,
        tid("callback"),
        Token::Colon,
        tid("ptr"),
        Token::Equals,
        Token::Addr,
        tid("missing"),
        tid("main"),
        Token::Colon,
        Token::LBrace,
        Token::RBrace,
    ])
    .unwrap();

    let error = validate_program_symbols(&program).unwrap_err();

    assert_eq!(
        error,
        "Unknown address target \"missing\" in memory declaration \"callback\""
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
fn parses_typed_intrinsic_calls() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        tid("min"),
        Token::LParen,
        treg("rbx"),
        Token::Comma,
        treg("rcx"),
        Token::RParen,
        Token::Colon,
        tid("u64"),
        treg("xmm0"),
        Token::Equals,
        tid("sqrt"),
        Token::LParen,
        treg("xmm1"),
        Token::RParen,
        Token::Colon,
        tid("f64"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![
            Instruction::Assign {
                dst: AssignmentTarget::Operand(reg("rax")),
                value: AssignmentValue::IntrinsicCall {
                    op: IntrinsicOp::Min,
                    width: MemoryWidth::U64,
                    args: vec![reg("rbx"), reg("rcx")],
                },
            },
            Instruction::Assign {
                dst: AssignmentTarget::Operand(reg("xmm0")),
                value: AssignmentValue::IntrinsicCall {
                    op: IntrinsicOp::Sqrt,
                    width: MemoryWidth::F64,
                    args: vec![reg("xmm1")],
                },
            },
        ]
    );
}

#[test]
fn parses_rounding_typed_intrinsic_calls() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("xmm0"),
        Token::Equals,
        tid("round"),
        Token::LParen,
        treg("xmm1"),
        Token::RParen,
        Token::Colon,
        tid("f64"),
        treg("xmm2"),
        Token::Equals,
        tid("floor"),
        Token::LParen,
        treg("xmm3"),
        Token::RParen,
        Token::Colon,
        tid("f32"),
        treg("xmm4"),
        Token::Equals,
        tid("ceil"),
        Token::LParen,
        treg("xmm5"),
        Token::RParen,
        Token::Colon,
        tid("f64"),
        treg("xmm6"),
        Token::Equals,
        tid("trunc"),
        Token::LParen,
        treg("xmm7"),
        Token::RParen,
        Token::Colon,
        tid("f32"),
    ]);

    let program = parse(finish_label(tokens)).unwrap();

    assert_eq!(
        program.labels[0].instructions,
        vec![
            Instruction::Assign {
                dst: AssignmentTarget::Operand(reg("xmm0")),
                value: AssignmentValue::IntrinsicCall {
                    op: IntrinsicOp::Round,
                    width: MemoryWidth::F64,
                    args: vec![reg("xmm1")],
                },
            },
            Instruction::Assign {
                dst: AssignmentTarget::Operand(reg("xmm2")),
                value: AssignmentValue::IntrinsicCall {
                    op: IntrinsicOp::Floor,
                    width: MemoryWidth::F32,
                    args: vec![reg("xmm3")],
                },
            },
            Instruction::Assign {
                dst: AssignmentTarget::Operand(reg("xmm4")),
                value: AssignmentValue::IntrinsicCall {
                    op: IntrinsicOp::Ceil,
                    width: MemoryWidth::F64,
                    args: vec![reg("xmm5")],
                },
            },
            Instruction::Assign {
                dst: AssignmentTarget::Operand(reg("xmm6")),
                value: AssignmentValue::IntrinsicCall {
                    op: IntrinsicOp::Trunc,
                    width: MemoryWidth::F32,
                    args: vec![reg("xmm7")],
                },
            },
        ]
    );
}

#[test]
fn rejects_unknown_typed_intrinsic_call() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        tid("clamp"),
        Token::LParen,
        treg("rbx"),
        Token::RParen,
        Token::Colon,
        tid("u64"),
    ]);

    let error = parse(finish_label(tokens)).unwrap_err();

    assert_eq!(error, "Unknown typed intrinsic call \"clamp\"");
}

#[test]
fn rejects_typed_intrinsic_call_arity_mismatch() {
    let mut tokens = empty_main_prefix();
    tokens.extend([
        treg("rax"),
        Token::Equals,
        tid("min"),
        Token::LParen,
        treg("rbx"),
        Token::RParen,
        Token::Colon,
        tid("u64"),
    ]);

    let error = parse(finish_label(tokens)).unwrap_err();

    assert_eq!(
        error,
        "Typed intrinsic call min expects 2 argument(s), found 1"
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
