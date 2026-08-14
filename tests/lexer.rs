use subsea::grammar::Token;
use subsea::lexer::get_next_token;

fn lex_one(source: &str) -> Result<Option<Token>, String> {
    get_next_token(&mut source.chars().peekable())
}

fn lex_all(source: &str) -> Result<Vec<Token>, String> {
    let mut chars = source.chars().peekable();
    let mut tokens = Vec::new();

    while let Some(token) = get_next_token(&mut chars)? {
        tokens.push(token);
    }

    Ok(tokens)
}

fn s(value: &str) -> String {
    value.to_string()
}

#[test]
fn lexes_local_identifier() {
    assert_eq!(
        lex_one(".loop").unwrap(),
        Some(Token::LocalIdent(s("loop")))
    );
}

#[test]
fn rejects_bare_period() {
    let error = lex_one(".").unwrap_err();

    assert_eq!(error, "Expected local identifier after '.'");
}

#[test]
fn rejects_non_ascii_identifier_start() {
    let error = lex_one("étiquette").unwrap_err();

    assert_eq!(error, "Unknown character 'é'");
}

#[test]
fn lexes_float_number_literal() {
    assert_eq!(lex_one("1.5").unwrap(), Some(Token::FloatLiteral(s("1.5"))));
}

#[test]
fn lexes_hex_number_literal() {
    assert_eq!(
        lex_one("0xf6b8f4b39de7d1ae").unwrap(),
        Some(Token::NumberLiteral(s("0xf6b8f4b39de7d1ae")))
    );
}

#[test]
fn lexes_comparison_operators() {
    for (source, token) in [
        ("==", Token::EqualsEquals),
        ("!=", Token::NotEquals),
        ("<", Token::Less),
        ("<=", Token::LessEquals),
        (">", Token::Greater),
        (">=", Token::GreaterEquals),
        ("i<", Token::ILess),
        ("i<=", Token::ILessEquals),
        ("i>", Token::IGreater),
        ("i>=", Token::IGreaterEquals),
        ("u<", Token::ULess),
        ("u<=", Token::ULessEquals),
        ("u>", Token::UGreater),
        ("u>=", Token::UGreaterEquals),
    ] {
        assert_eq!(lex_one(source).unwrap(), Some(token));
    }
}

#[test]
fn lexes_float_arithmetic_operators() {
    for (source, token) in [
        ("f32+", Token::F32Plus),
        ("f32-", Token::F32Minus),
        ("f32*", Token::F32Star),
        ("f32/", Token::F32Slash),
        ("f64+", Token::F64Plus),
        ("f64-", Token::F64Minus),
        ("f64*", Token::F64Star),
        ("f64/", Token::F64Slash),
    ] {
        assert_eq!(lex_one(source).unwrap(), Some(token));
    }
}

#[test]
fn lexes_bitwise_operators() {
    for (source, token) in [
        ("&", Token::Ampersand),
        ("|", Token::Pipe),
        ("^", Token::Caret),
        ("~", Token::Tilde),
        ("<<", Token::ShiftLeft),
        (">>", Token::ShiftRight),
        ("i>>", Token::IShiftRight),
    ] {
        assert_eq!(lex_one(source).unwrap(), Some(token));
    }
}

#[test]
fn lexes_double_colon() {
    assert_eq!(lex_one("::").unwrap(), Some(Token::DoubleColon));
}

#[test]
fn lexes_bitwise_and_without_spaces() {
    assert_eq!(
        lex_all("rbx&rcx").unwrap(),
        vec![
            Token::Register(s("rbx")),
            Token::Ampersand,
            Token::Register(s("rcx")),
        ]
    );
}

#[test]
fn lexes_float_comparison_operators() {
    for (source, token) in [
        ("f32<", Token::F32Less),
        ("f32<=", Token::F32LessEquals),
        ("f32>", Token::F32Greater),
        ("f32>=", Token::F32GreaterEquals),
        ("f32==", Token::F32EqualsEquals),
        ("f32!=", Token::F32NotEquals),
        ("f64<", Token::F64Less),
        ("f64>=", Token::F64GreaterEquals),
    ] {
        assert_eq!(lex_one(source).unwrap(), Some(token));
    }
}

#[test]
fn lexes_storage_and_cleanup_keywords() {
    for (source, token) in [
        ("const", Token::Const),
        ("from", Token::From),
        ("hlt", Token::Halt),
        ("in", Token::In),
        ("import", Token::Import),
        ("out", Token::Out),
        ("read", Token::Read),
        ("slice", Token::Slice),
        ("stack", Token::Stack),
        ("stdin", Token::Stdin),
        ("x86", Token::X86),
    ] {
        assert_eq!(lex_one(source).unwrap(), Some(token));
    }
}

#[test]
fn lexes_namespaced_linux_instruction_parts() {
    assert_eq!(
        lex_all("linux.print").unwrap(),
        vec![Token::Ident(s("linux")), Token::LocalIdent(s("print"))]
    );
}

#[test]
fn lexes_raw_x86_instruction_parts() {
    assert_eq!(
        lex_all("x86 \"out 0xe9, al\"").unwrap(),
        vec![Token::X86, Token::Text(s("out 0xe9, al"))]
    );
}

#[test]
fn lexes_xmm_register() {
    assert_eq!(lex_one("xmm15").unwrap(), Some(Token::Register(s("xmm15"))));
}
