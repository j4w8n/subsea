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

                        while let Some(next_char) = chars.next() {
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
                Some(Token::Directive(s))
            }
            _ => Some(Token::Period),
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
        Some('=') => Some(Token::Equals),
        Some('(') => Some(Token::LParen),
        Some(')') => Some(Token::RParen),
        Some('{') => Some(Token::LBrace),
        Some('}') => Some(Token::RBrace),
        Some('[') => Some(Token::LBracket),
        Some(']') => Some(Token::RBracket),
        Some('$') => Some(Token::Dollar),
        Some('%') => Some(Token::Percent),
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
        Some(c) if c.is_ascii_digit() => {
            let num_str = lex_number(c, chars);
            Some(Token::NumberLiteral(num_str))
        }
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
                "true" => Some(Token::Bool(true)),
                "false" => Some(Token::Bool(false)),
                "i" if matches!(chars.peek(), Some('*')) => {
                    chars.next();
                    Some(Token::IStar)
                }
                "i" if matches!(chars.peek(), Some('/')) => {
                    chars.next();
                    Some(Token::ISlash)
                }
                "call" => Some(Token::Call),
                "exit" => Some(Token::Exit),
                "jmp" => Some(Token::Jmp),
                "let" => Some(Token::Let),
                "mem" => Some(Token::Mem),
                "print" => Some(Token::Print),
                "ret" => Some(Token::Ret),
                "syscall" => Some(Token::Syscall),
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
    c == '_' || c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

fn lex_number(first: char, chars: &mut Peekable<Chars<'_>>) -> String {
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

        if let Some(next_after_dot) = clone.peek() {
            if next_after_dot.is_ascii_digit() {
                num_str.push(chars.next().unwrap());

                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() {
                        num_str.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
            }
        }
    }

    num_str
}

fn is_register(s: &str) -> bool {
    matches!(
        s,
        "rax"
            | "rbx"
            | "rcx"
            | "rdx"
            | "rdi"
            | "rsi"
            | "rbp"
            | "rsp"
            | "eax"
            | "ebx"
            | "ecx"
            | "edx"
            | "edi"
            | "esi"
            | "ebp"
            | "esp"
            | "ax"
            | "bx"
            | "cx"
            | "dx"
            | "di"
            | "si"
            | "bp"
            | "sp"
            | "al"
            | "bl"
            | "cl"
            | "dl"
            | "ah"
            | "bh"
            | "ch"
            | "dh"
            | "dil"
            | "sil"
            | "bpl"
            | "spl"
            | "r8"
            | "r9"
            | "r10"
            | "r11"
            | "r12"
            | "r13"
            | "r14"
            | "r15"
            | "r8d"
            | "r9d"
            | "r10d"
            | "r11d"
            | "r12d"
            | "r13d"
            | "r14d"
            | "r15d"
            | "r8w"
            | "r9w"
            | "r10w"
            | "r11w"
            | "r12w"
            | "r13w"
            | "r14w"
            | "r15w"
            | "r8b"
            | "r9b"
            | "r10b"
            | "r11b"
            | "r12b"
            | "r13b"
            | "r14b"
            | "r15b"
    )
}
