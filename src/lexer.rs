use crate::grammar::Token;
use std::iter::Peekable;
use std::str::Chars;

pub fn get_next_token(chars: &mut Peekable<Chars>) -> Result<Option<Token>, String> {
    loop {
        match chars.peek() {
            Some(c) if c.is_whitespace() => {
                chars.next();
            }
            Some('/') => {
                let mut clone = chars.clone();
                clone.next();

                match clone.peek() {
                    Some('/') => {
                        // skip single-line comments
                        chars.next(); // /
                        chars.next(); // /

                        while let Some(&next_char) = chars.peek() {
                            if next_char == '\n' {
                                break;
                            }
                            chars.next();
                        }
                    }
                    Some('*') => {
                        // skip multi-line comments
                        chars.next(); // /
                        chars.next(); // *

                        let mut prev_was_star = false;
                        let mut closed = false;

                        for next_char in chars.by_ref() {
                            if prev_was_star && next_char == '/' {
                                closed = true;
                                break;
                            }

                            prev_was_star = next_char == '*';
                        }

                        if !closed {
                            return Err(String::from("Unterminated multiline comment"));
                        }
                    }
                    _ => break,
                }
            }
            _ => break,
        }
    }

    let char = chars.next();
    let token = match char {
        Some('.') => match chars.peek() {
            Some(&c) if is_ident_start(c) => Some(Token::LocalIdent(lex_ident(chars))),
            _ => return Err(String::from("Expected local identifier after '.'")),
        },
        Some('&') => Some(Token::Ampersand),
        Some('|') => Some(Token::Pipe),
        Some('^') => Some(Token::Caret),
        Some('%') => Some(Token::Percent),
        Some('~') => Some(Token::Tilde),
        Some('+') => Some(Token::Plus),
        Some('-') => Some(Token::Minus),
        Some('*') => {
            if chars.peek() == Some(&'*') {
                chars.next();
                Some(Token::DoubleStar)
            } else {
                Some(Token::Star)
            }
        }
        Some('/') => Some(Token::Slash),
        Some('=') => {
            if chars.peek() == Some(&'=') {
                chars.next();
                Some(Token::EqualsEquals)
            } else {
                Some(Token::Equals)
            }
        }
        Some('!') => {
            if chars.peek() == Some(&'=') {
                chars.next();
                Some(Token::NotEquals)
            } else {
                return Err(String::from("Expected '=' after '!'"));
            }
        }
        Some('<') => {
            if chars.peek() == Some(&'<') {
                chars.next();
                Some(Token::ShiftLeft)
            } else if chars.peek() == Some(&'=') {
                chars.next();
                Some(Token::LessEquals)
            } else {
                Some(Token::Less)
            }
        }
        Some('>') => {
            if chars.peek() == Some(&'>') {
                chars.next();
                Some(Token::ShiftRight)
            } else if chars.peek() == Some(&'=') {
                chars.next();
                Some(Token::GreaterEquals)
            } else {
                Some(Token::Greater)
            }
        }
        Some('(') => Some(Token::LParen),
        Some(')') => Some(Token::RParen),
        Some('{') => Some(Token::LBrace),
        Some('}') => Some(Token::RBrace),
        Some('[') => Some(Token::LBracket),
        Some(']') => Some(Token::RBracket),
        Some(':') => {
            if chars.peek() == Some(&':') {
                chars.next();
                Some(Token::DoubleColon)
            } else {
                Some(Token::Colon)
            }
        }
        Some(',') => Some(Token::Comma),
        Some('"') => {
            let mut value = String::new();
            while let Some(next_char) = chars.next() {
                match next_char {
                    '"' => return Ok(Some(Token::Text(value))),
                    '\\' => match chars.next() {
                        Some('n') => value.push('\n'),
                        Some('t') => value.push('\t'),
                        Some('"') => value.push('"'),
                        Some('\\') => value.push('\\'),
                        Some(escaped) => return Err(format!("Unknown string escape \\{escaped}")),
                        None => return Err(String::from("Unterminated string escape")),
                    },
                    _ => value.push(next_char),
                }
            }

            return Err(String::from("Unterminated string literal"));
        }
        Some(c) if c.is_ascii_digit() => Some(lex_number(c, chars)?),
        Some(c) if is_ident_start(c) => {
            let s = lex_ident_after(c, chars);

            match s.as_str() {
                "if" => Some(Token::If),
                "i" => prefixed_integer_operator(
                    chars,
                    Token::ILess,
                    Token::ILessEquals,
                    Token::IGreater,
                    Token::IGreaterEquals,
                    Token::IStar,
                    Token::ISlash,
                    Token::IPercent,
                )
                .or_else(|| Some(Token::Ident(s))),
                "call" => Some(Token::Call),
                "const" => Some(Token::Const),
                "addr" => Some(Token::Addr),
                "align" => Some(Token::Align),
                "data" => Some(Token::Data),
                "exit" => Some(Token::Exit),
                "export" => Some(Token::Export),
                "f32" => prefixed_float_operator(chars, MemoryWidthTokens::F32)
                    .or_else(|| Some(Token::Ident(s))),
                "f64" => prefixed_float_operator(chars, MemoryWidthTokens::F64)
                    .or_else(|| Some(Token::Ident(s))),
                "from" => Some(Token::From),
                "hlt" => Some(Token::Halt),
                "in" => Some(Token::In),
                "import" => Some(Token::Import),
                "jmp" => Some(Token::Jmp),
                "keep" => Some(Token::Keep),
                "mem" => Some(Token::Mem),
                "nop" => Some(Token::Nop),
                "out" => Some(Token::Out),
                "pop" => Some(Token::Pop),
                "print" => Some(Token::Print),
                "push" => Some(Token::Push),
                "read" => Some(Token::Read),
                "ret" => Some(Token::Ret),
                "section" => Some(Token::Section),
                "slice" => Some(Token::Slice),
                "stack" => Some(Token::Stack),
                "stdin" => Some(Token::Stdin),
                "syscall" => Some(Token::Syscall),
                "x86" => Some(Token::X86),
                "zero" => Some(Token::Zero),
                "u" => prefixed_integer_operator(
                    chars,
                    Token::ULess,
                    Token::ULessEquals,
                    Token::UGreater,
                    Token::UGreaterEquals,
                    Token::UStar,
                    Token::USlash,
                    Token::UPercent,
                )
                .or_else(|| Some(Token::Ident(s))),
                register if is_register(register) => Some(Token::Register(s)),
                _ => Some(Token::Ident(s)),
            }
        }
        None => None,
        Some(char) => return Err(format!("Unknown character {char:?}")),
    };

    Ok(token)
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

fn lex_ident(chars: &mut Peekable<Chars<'_>>) -> String {
    let first = chars.next().unwrap();
    lex_ident_after(first, chars)
}

fn lex_ident_after(first: char, chars: &mut Peekable<Chars<'_>>) -> String {
    let mut value = String::from(first);

    while let Some(&next_char) = chars.peek() {
        if is_ident_continue(next_char) {
            value.push(chars.next().unwrap());
        } else {
            break;
        }
    }

    value
}

fn prefixed_integer_operator(
    chars: &mut Peekable<Chars<'_>>,
    less: Token,
    less_equals: Token,
    greater: Token,
    greater_equals: Token,
    star: Token,
    slash: Token,
    percent: Token,
) -> Option<Token> {
    match chars.peek() {
        Some('>') if matches!(greater, Token::IGreater) => {
            let mut clone = chars.clone();
            clone.next();
            if clone.peek() == Some(&'>') {
                chars.next();
                chars.next();
                Some(Token::IShiftRight)
            } else {
                Some(prefixed_comparison(chars, greater, greater_equals))
            }
        }
        Some('<') => Some(prefixed_comparison(chars, less, less_equals)),
        Some('>') => Some(prefixed_comparison(chars, greater, greater_equals)),
        Some('*') => {
            chars.next();
            Some(star)
        }
        Some('/') => {
            chars.next();
            Some(slash)
        }
        Some('%') => {
            chars.next();
            Some(percent)
        }
        _ => None,
    }
}

fn prefixed_comparison(chars: &mut Peekable<Chars<'_>>, plain: Token, equals: Token) -> Token {
    chars.next();
    if chars.peek() == Some(&'=') {
        chars.next();
        equals
    } else {
        plain
    }
}

#[derive(Clone, Copy)]
enum MemoryWidthTokens {
    F32,
    F64,
}

fn prefixed_float_operator(
    chars: &mut Peekable<Chars<'_>>,
    width: MemoryWidthTokens,
) -> Option<Token> {
    let token = match (width, chars.peek()) {
        (MemoryWidthTokens::F32, Some('+')) => Token::F32Plus,
        (MemoryWidthTokens::F32, Some('-')) => Token::F32Minus,
        (MemoryWidthTokens::F32, Some('*')) => Token::F32Star,
        (MemoryWidthTokens::F32, Some('/')) => Token::F32Slash,
        (MemoryWidthTokens::F32, Some('<')) => {
            return Some(prefixed_comparison(
                chars,
                Token::F32Less,
                Token::F32LessEquals,
            ));
        }
        (MemoryWidthTokens::F32, Some('>')) => {
            return Some(prefixed_comparison(
                chars,
                Token::F32Greater,
                Token::F32GreaterEquals,
            ));
        }
        (MemoryWidthTokens::F64, Some('+')) => Token::F64Plus,
        (MemoryWidthTokens::F64, Some('-')) => Token::F64Minus,
        (MemoryWidthTokens::F64, Some('*')) => Token::F64Star,
        (MemoryWidthTokens::F64, Some('/')) => Token::F64Slash,
        (MemoryWidthTokens::F64, Some('<')) => {
            return Some(prefixed_comparison(
                chars,
                Token::F64Less,
                Token::F64LessEquals,
            ));
        }
        (MemoryWidthTokens::F64, Some('>')) => {
            return Some(prefixed_comparison(
                chars,
                Token::F64Greater,
                Token::F64GreaterEquals,
            ));
        }
        (MemoryWidthTokens::F32, Some('=')) => {
            return prefixed_equals(chars, Token::F32EqualsEquals);
        }
        (MemoryWidthTokens::F32, Some('!')) => return prefixed_equals(chars, Token::F32NotEquals),
        (MemoryWidthTokens::F64, Some('=')) => {
            return prefixed_equals(chars, Token::F64EqualsEquals);
        }
        (MemoryWidthTokens::F64, Some('!')) => return prefixed_equals(chars, Token::F64NotEquals),
        _ => return None,
    };

    chars.next();
    Some(token)
}

fn prefixed_equals(chars: &mut Peekable<Chars<'_>>, token: Token) -> Option<Token> {
    chars.next();
    if chars.peek() == Some(&'=') {
        chars.next();
        Some(token)
    } else {
        None
    }
}

fn lex_number(first: char, chars: &mut Peekable<Chars<'_>>) -> Result<Token, String> {
    let mut num_str = String::from(first);

    if first == '0' && matches!(chars.peek(), Some('x' | 'X')) {
        num_str.push(chars.next().unwrap());

        let mut has_digits = false;
        while let Some(&c) = chars.peek() {
            if c.is_ascii_hexdigit() {
                has_digits = true;
                num_str.push(chars.next().unwrap());
            } else {
                break;
            }
        }

        if !has_digits {
            return Err(String::from("Expected hexadecimal digits after 0x"));
        }

        return Ok(Token::NumberLiteral(num_str));
    }

    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            num_str.push(chars.next().unwrap());
        } else {
            break;
        }
    }

    if let Some(&'.') = chars.peek() {
        let mut clone = chars.clone();
        clone.next();

        if let Some(next_after_dot) = clone.peek()
            && next_after_dot.is_ascii_digit()
        {
            num_str.push(chars.next().unwrap());

            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() {
                    num_str.push(chars.next().unwrap());
                } else {
                    break;
                }
            }

            return Ok(Token::FloatLiteral(num_str));
        }
    }

    Ok(Token::NumberLiteral(num_str))
}

fn is_register(s: &str) -> bool {
    crate::register::is_register(s)
}
