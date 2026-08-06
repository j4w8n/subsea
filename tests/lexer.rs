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
