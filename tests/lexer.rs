use subsea::grammar::Token;
use subsea::lexer::get_next_token;

fn lex_one(source: &str) -> Result<Option<Token>, String> {
    get_next_token(&mut source.chars().peekable())
}

#[test]
fn lexes_local_identifier() {
    assert_eq!(
        lex_one(".loop").unwrap(),
        Some(Token::LocalIdent(String::from("loop")))
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
    assert_eq!(
        lex_one("1.5").unwrap(),
        Some(Token::FloatLiteral(String::from("1.5")))
    );
}

#[test]
fn lexes_comparison_operators() {
    assert_eq!(lex_one("==").unwrap(), Some(Token::EqualsEquals));
    assert_eq!(lex_one("!=").unwrap(), Some(Token::NotEquals));
    assert_eq!(lex_one("<").unwrap(), Some(Token::Less));
    assert_eq!(lex_one("<=").unwrap(), Some(Token::LessEquals));
    assert_eq!(lex_one(">").unwrap(), Some(Token::Greater));
    assert_eq!(lex_one(">=").unwrap(), Some(Token::GreaterEquals));
    assert_eq!(lex_one("i<").unwrap(), Some(Token::ILess));
    assert_eq!(lex_one("i<=").unwrap(), Some(Token::ILessEquals));
    assert_eq!(lex_one("i>").unwrap(), Some(Token::IGreater));
    assert_eq!(lex_one("i>=").unwrap(), Some(Token::IGreaterEquals));
    assert_eq!(lex_one("u<").unwrap(), Some(Token::ULess));
    assert_eq!(lex_one("u<=").unwrap(), Some(Token::ULessEquals));
    assert_eq!(lex_one("u>").unwrap(), Some(Token::UGreater));
    assert_eq!(lex_one("u>=").unwrap(), Some(Token::UGreaterEquals));
}

#[test]
fn lexes_float_arithmetic_operators() {
    assert_eq!(lex_one("f32+").unwrap(), Some(Token::F32Plus));
    assert_eq!(lex_one("f32-").unwrap(), Some(Token::F32Minus));
    assert_eq!(lex_one("f32*").unwrap(), Some(Token::F32Star));
    assert_eq!(lex_one("f32/").unwrap(), Some(Token::F32Slash));
    assert_eq!(lex_one("f64+").unwrap(), Some(Token::F64Plus));
    assert_eq!(lex_one("f64-").unwrap(), Some(Token::F64Minus));
    assert_eq!(lex_one("f64*").unwrap(), Some(Token::F64Star));
    assert_eq!(lex_one("f64/").unwrap(), Some(Token::F64Slash));
}

#[test]
fn lexes_storage_and_cleanup_keywords() {
    assert_eq!(lex_one("const").unwrap(), Some(Token::Const));
    assert_eq!(lex_one("stack").unwrap(), Some(Token::Stack));
}

#[test]
fn lexes_xmm_register() {
    assert_eq!(
        lex_one("xmm15").unwrap(),
        Some(Token::Register(String::from("xmm15")))
    );
}
