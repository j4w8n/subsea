use crate::diagnostic::{SourceId, SourceMap, Span};
use crate::grammar::Token;
use crate::lexer::{get_next_token, lex_with_spans};

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
fn tracks_token_spans() {
    let tokens = lex_with_spans("rax = rdx", SourceId(0)).unwrap();

    assert_eq!(tokens[0].token, Token::Register(s("rax")));
    assert_eq!(tokens[0].span, Span::new(SourceId(0), 0, 3));
    assert_eq!(tokens[1].token, Token::Equals);
    assert_eq!(tokens[1].span, Span::new(SourceId(0), 4, 5));
    assert_eq!(tokens[2].token, Token::Register(s("rdx")));
    assert_eq!(tokens[2].span, Span::new(SourceId(0), 6, 9));
}

#[test]
fn renders_a_source_diagnostic() {
    let mut sources = SourceMap::default();
    let source = "rax = sqrt(9:u64\n";
    let source_id = sources.add("main.ss", source);
    let diagnostic = crate::diagnostic::Diagnostic::new("expected ')' after intrinsic argument")
        .at(Span::new(source_id, 15, 15));

    assert_eq!(
        diagnostic.render(&sources),
        "error: expected ')' after intrinsic argument\n --> main.ss:1:16\n  |\n1 | rax = sqrt(9:u64\n  |                ^"
    );
}

#[test]
fn renders_unicode_columns_and_multiline_spans_safely() {
    let mut sources = SourceMap::default();
    let source = "// café\nrax = rdx\n";
    let source_id = sources.add("main.ss", source);
    let diagnostic =
        crate::diagnostic::Diagnostic::new("invalid instruction").at(Span::new(source_id, 9, 18));

    assert_eq!(
        diagnostic.render(&sources),
        "error: invalid instruction\n --> main.ss:2:1\n  |\n2 | rax = rdx\n  | ^~~~~~~~~"
    );
}

#[test]
fn renders_span_ending_inside_unicode_character_safely() {
    let mut sources = SourceMap::default();
    let source_id = sources.add("main.ss", "aéz\n");
    let diagnostic =
        crate::diagnostic::Diagnostic::new("invalid text").at(Span::new(source_id, 0, 2));

    assert_eq!(
        diagnostic.render(&sources),
        "error: invalid text\n --> main.ss:1:1\n  |\n1 | aéz\n  | ^"
    );
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
        ("%", Token::Percent),
        ("~", Token::Tilde),
        ("**", Token::DoubleStar),
        ("<<", Token::ShiftLeft),
        (">>", Token::ShiftRight),
        ("i>>", Token::IShiftRight),
        ("i%", Token::IPercent),
        ("u%", Token::UPercent),
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
        ("hlt", Token::Ident(String::from("hlt"))),
        ("in", Token::In),
        ("import", Token::Import),
        ("nop", Token::Nop),
        ("out", Token::Out),
        ("read", Token::Read),
        ("repeat", Token::Repeat),
        ("slice", Token::Slice),
        ("stack", Token::Stack),
        ("stdin", Token::Stdin),
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
fn lexes_namespaced_x86_assembly_parts() {
    assert_eq!(
        lex_all("asm.x86 \"out 0xe9, al\"").unwrap(),
        vec![
            Token::Ident(s("asm")),
            Token::LocalIdent(s("x86")),
            Token::Text(s("out 0xe9, al")),
        ]
    );
}

#[test]
fn lexes_xmm_register() {
    assert_eq!(lex_one("xmm15").unwrap(), Some(Token::Register(s("xmm15"))));
}
