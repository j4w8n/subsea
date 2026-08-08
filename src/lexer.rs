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
            Some(&c) if is_ident_start(c) => {
                let mut s = String::new();
                s.push(chars.next().unwrap());
                while let Some(&next_char) = chars.peek() {
                    if is_ident_continue(next_char) {
                        s.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                Some(Token::LocalIdent(s))
            }
            _ => return Err(String::from("Expected local identifier after '.'")),
        },
        Some('&') => match chars.peek() {
            Some(&c) if is_ident_start(c) => {
                let mut s = String::new();
                s.push(chars.next().unwrap());
                while let Some(&next_char) = chars.peek() {
                    if is_ident_continue(next_char) {
                        s.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                Some(Token::Pointer(s))
            }
            _ => Some(Token::Ampersand),
        },
        Some('+') => Some(Token::Plus),
        Some('-') => Some(Token::Minus),
        Some('*') => Some(Token::Star),
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
            if chars.peek() == Some(&'=') {
                chars.next();
                Some(Token::LessEquals)
            } else {
                Some(Token::Less)
            }
        }
        Some('>') => {
            if chars.peek() == Some(&'=') {
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
        Some(':') => Some(Token::Colon),
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
            let mut s = String::from(c);

            while let Some(&next_char) = chars.peek() {
                if is_ident_continue(next_char) {
                    s.push(chars.next().unwrap());
                } else {
                    break;
                }
            }

            match s.as_str() {
                "if" => Some(Token::If),
                "i" if matches!(chars.peek(), Some('<')) => {
                    chars.next();
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        Some(Token::ILessEquals)
                    } else {
                        Some(Token::ILess)
                    }
                }
                "i" if matches!(chars.peek(), Some('>')) => {
                    chars.next();
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        Some(Token::IGreaterEquals)
                    } else {
                        Some(Token::IGreater)
                    }
                }
                "i" if matches!(chars.peek(), Some('*')) => {
                    chars.next();
                    Some(Token::IStar)
                }
                "i" if matches!(chars.peek(), Some('/')) => {
                    chars.next();
                    Some(Token::ISlash)
                }
                "call" => Some(Token::Call),
                "const" => Some(Token::Const),
                "exit" => Some(Token::Exit),
                "f32" if matches!(chars.peek(), Some('+')) => {
                    chars.next();
                    Some(Token::F32Plus)
                }
                "f32" if matches!(chars.peek(), Some('-')) => {
                    chars.next();
                    Some(Token::F32Minus)
                }
                "f32" if matches!(chars.peek(), Some('*')) => {
                    chars.next();
                    Some(Token::F32Star)
                }
                "f32" if matches!(chars.peek(), Some('/')) => {
                    chars.next();
                    Some(Token::F32Slash)
                }
                "f64" if matches!(chars.peek(), Some('+')) => {
                    chars.next();
                    Some(Token::F64Plus)
                }
                "f64" if matches!(chars.peek(), Some('-')) => {
                    chars.next();
                    Some(Token::F64Minus)
                }
                "f64" if matches!(chars.peek(), Some('*')) => {
                    chars.next();
                    Some(Token::F64Star)
                }
                "f64" if matches!(chars.peek(), Some('/')) => {
                    chars.next();
                    Some(Token::F64Slash)
                }
                "jmp" => Some(Token::Jmp),
                "mem" => Some(Token::Mem),
                "pop" => Some(Token::Pop),
                "print" => Some(Token::Print),
                "push" => Some(Token::Push),
                "ret" => Some(Token::Ret),
                "slice" => Some(Token::Slice),
                "stack" => Some(Token::Stack),
                "syscall" => Some(Token::Syscall),
                "u" if matches!(chars.peek(), Some('<')) => {
                    chars.next();
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        Some(Token::ULessEquals)
                    } else {
                        Some(Token::ULess)
                    }
                }
                "u" if matches!(chars.peek(), Some('>')) => {
                    chars.next();
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        Some(Token::UGreaterEquals)
                    } else {
                        Some(Token::UGreater)
                    }
                }
                "u" if matches!(chars.peek(), Some('*')) => {
                    chars.next();
                    Some(Token::UStar)
                }
                "u" if matches!(chars.peek(), Some('/')) => {
                    chars.next();
                    Some(Token::USlash)
                }
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

fn lex_number(first: char, chars: &mut Peekable<Chars<'_>>) -> Result<Token, String> {
    let mut num_str = String::from(first);

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
