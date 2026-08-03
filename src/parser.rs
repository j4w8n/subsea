use crate::ast::{
    Address, AddressOperator, AddressTerm, Instruction, Label, MemoryWidth, Operand, Program,
};
use crate::grammar::Token;

pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    pub fn parse_program(&mut self) -> Result<Program, String> {
        let entry = self.parse_entry_directive()?;
        let mut labels = Vec::new();

        while !self.is_at_end() {
            labels.push(self.parse_label()?);
        }

        Ok(Program { entry, labels })
    }

    fn parse_entry_directive(&mut self) -> Result<String, String> {
        match self.advance() {
            Some(Token::Directive(name)) if name == "entry" => {}
            Some(token) => return Err(format!("Expected .entry directive, found {token:?}")),
            None => {
                return Err(String::from(
                    "Expected .entry directive, found end of input",
                ));
            }
        }

        match self.advance() {
            Some(Token::Ident(name)) => Ok(name),
            Some(token) => Err(format!("Expected entry label name, found {token:?}")),
            None => Err(String::from(
                "Expected entry label name, found end of input",
            )),
        }
    }

    fn parse_label(&mut self) -> Result<Label, String> {
        let name = match self.advance() {
            Some(Token::Ident(name)) => name,
            Some(token) => return Err(format!("Expected label name, found {token:?}")),
            None => return Err(String::from("Expected label name, found end of input")),
        };

        self.expect(Token::Colon, "Expected ':' after label name")?;
        self.expect(Token::LBrace, "Expected '{' to start label block")?;

        let mut instructions = Vec::new();
        while !matches!(self.peek(), Some(Token::RBrace)) {
            if self.is_at_end() {
                return Err(format!("Expected '}}' to close label '{name}'"));
            }

            instructions.push(self.parse_instruction()?);
        }

        self.expect(Token::RBrace, "Expected '}' after label block")?;

        Ok(Label { name, instructions })
    }

    fn parse_instruction(&mut self) -> Result<Instruction, String> {
        match self.advance() {
            Some(Token::Add) => {
                let (src, dst) = self.parse_binary_operands("add")?;
                Ok(Instruction::Add { src, dst })
            }
            Some(Token::Copy) => {
                let (src, dst) = self.parse_binary_operands("copy")?;
                Ok(Instruction::Copy { src, dst })
            }
            Some(Token::Idiv) => {
                let divisor = self.parse_operand()?;
                Ok(Instruction::Idiv { divisor })
            }
            Some(Token::Imul) => {
                let (src, dst) = self.parse_binary_operands("imul")?;
                Ok(Instruction::Imul { src, dst })
            }
            Some(Token::Jmp) => match self.advance() {
                Some(Token::Ident(target)) => Ok(Instruction::Jmp { target }),
                Some(token) => Err(format!("Expected jump target label, found {token:?}")),
                None => Err(String::from(
                    "Expected jump target label, found end of input",
                )),
            },
            Some(Token::Sub) => {
                let (src, dst) = self.parse_binary_operands("sub")?;
                Ok(Instruction::Sub { src, dst })
            }
            Some(Token::Syscall) => Ok(Instruction::Syscall),
            Some(Token::Udiv) => {
                let divisor = self.parse_operand()?;
                Ok(Instruction::Udiv { divisor })
            }
            Some(Token::Umul) => {
                let (src, dst) = self.parse_binary_operands("umul")?;
                Ok(Instruction::Umul { src, dst })
            }
            Some(token) => Err(format!("Expected instruction, found {token:?}")),
            None => Err(String::from("Expected instruction, found end of input")),
        }
    }

    fn parse_binary_operands(&mut self, instruction: &str) -> Result<(Operand, Operand), String> {
        let src = self.parse_operand()?;
        self.expect(
            Token::Comma,
            &format!("Expected ',' after {instruction} source operand"),
        )?;
        let dst = self.parse_operand()?;

        Ok((src, dst))
    }

    fn parse_operand(&mut self) -> Result<Operand, String> {
        match self.advance() {
            Some(Token::Ampersand) => match self.advance() {
                Some(Token::Ident(name)) => Ok(Operand::Pointer(name)),
                Some(Token::Register(name)) => Err(format!(
                    "Cannot take the address of register {name}; expected a label after '&'"
                )),
                Some(Token::NumberLiteral(value)) => Err(format!(
                    "Cannot take the address of immediate value {value}; expected a label after '&'"
                )),
                Some(Token::LBracket) => Err(String::from(
                    "Cannot take the address of a dereference; '&[...]' is invalid syntax",
                )),
                Some(token) => Err(format!("Expected label after '&', found {token:?}")),
                None => Err(String::from("Expected label after '&', found end of input")),
            },
            Some(Token::LBracket) => {
                let address = self.parse_address()?;
                self.expect(Token::RBracket, "Expected ']' after memory operand")?;
                let width = self.parse_optional_memory_width()?;

                Ok(Operand::Dereference { address, width })
            }
            Some(Token::Minus) => match self.advance() {
                Some(Token::NumberLiteral(value)) => value
                    .parse::<i64>()
                    .map(|value| Operand::Immediate(-value))
                    .map_err(|_| format!("Invalid integer literal -{value}")),
                Some(token) => Err(format!("Expected number after '-', found {token:?}")),
                None => Err(String::from(
                    "Expected number after '-', found end of input",
                )),
            },
            Some(Token::NumberLiteral(value)) => value
                .parse::<i64>()
                .map(Operand::Immediate)
                .map_err(|_| format!("Invalid integer literal {value:?}")),
            Some(Token::Register(name)) => Ok(Operand::Register(name)),
            Some(Token::Ident(name)) => Ok(Operand::Ident(name)),
            Some(Token::Pointer(name)) => {
                if is_register_name(&name) {
                    Err(format!(
                        "Cannot take the address of register {name}; expected a label after '&'"
                    ))
                } else {
                    Ok(Operand::Pointer(name))
                }
            }
            Some(token) => Err(format!("Expected operand, found {token:?}")),
            None => Err(String::from("Expected operand, found end of input")),
        }
    }

    fn parse_optional_memory_width(&mut self) -> Result<Option<MemoryWidth>, String> {
        if !matches!(self.peek(), Some(Token::Colon)) {
            return Ok(None);
        }

        self.advance();

        match self.advance() {
            Some(Token::Ident(name)) => parse_memory_width(&name).map(Some),
            Some(token) => Err(format!("Expected memory width after ':', found {token:?}")),
            None => Err(String::from(
                "Expected memory width after ':', found end of input",
            )),
        }
    }

    fn parse_address(&mut self) -> Result<Address, String> {
        let first = self.parse_address_term()?;
        let mut rest = Vec::new();

        while matches!(self.peek(), Some(Token::Plus | Token::Minus)) {
            let operator = match self.advance() {
                Some(Token::Plus) => AddressOperator::Add,
                Some(Token::Minus) => AddressOperator::Subtract,
                _ => unreachable!(),
            };

            rest.push((operator, self.parse_address_term()?));
        }

        Ok(Address { first, rest })
    }

    fn parse_address_term(&mut self) -> Result<AddressTerm, String> {
        match self.advance() {
            Some(Token::NumberLiteral(value)) => {
                if matches!(self.peek(), Some(Token::Star)) {
                    return Err(String::from(
                        "Only registers can be scaled in memory operands",
                    ));
                }

                value
                    .parse::<i64>()
                    .map(AddressTerm::Immediate)
                    .map_err(|_| format!("Invalid integer literal {value:?}"))
            }
            Some(Token::Register(name)) => {
                if matches!(self.peek(), Some(Token::Star)) {
                    self.advance();

                    let scale = self.parse_address_scale()?;
                    Ok(AddressTerm::ScaledRegister {
                        register: name,
                        scale,
                    })
                } else {
                    Ok(AddressTerm::Register(name))
                }
            }
            Some(Token::Ident(name)) => {
                if matches!(self.peek(), Some(Token::Star)) {
                    return Err(String::from(
                        "Only registers can be scaled in memory operands",
                    ));
                }

                Ok(AddressTerm::Ident(name))
            }
            Some(Token::LBracket) => Err(String::from("Nested dereference is not supported yet")),
            Some(Token::Ampersand | Token::Pointer(_)) => Err(String::from(
                "Address-of syntax is not valid inside a memory operand",
            )),
            Some(token) => Err(format!("Expected address term, found {token:?}")),
            None => Err(String::from("Expected address term, found end of input")),
        }
    }

    fn parse_address_scale(&mut self) -> Result<i64, String> {
        let scale = match self.advance() {
            Some(Token::NumberLiteral(value)) => value
                .parse::<i64>()
                .map_err(|_| format!("Invalid address scale {value:?}"))?,
            Some(token) => {
                return Err(format!("Expected address scale after '*', found {token:?}"));
            }
            None => {
                return Err(String::from(
                    "Expected address scale after '*', found end of input",
                ));
            }
        };

        if matches!(scale, 1 | 2 | 4 | 8) {
            Ok(scale)
        } else {
            Err(format!(
                "Invalid address scale {scale}; expected one of 1, 2, 4, or 8"
            ))
        }
    }

    fn expect(&mut self, expected: Token, message: &str) -> Result<(), String> {
        match self.advance() {
            Some(token) if token == expected => Ok(()),
            Some(token) => Err(format!("{message}, found {token:?}")),
            None => Err(format!("{message}, found end of input")),
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position).cloned();
        self.position += usize::from(token.is_some());
        token
    }

    fn is_at_end(&self) -> bool {
        self.position >= self.tokens.len()
    }
}

fn parse_memory_width(name: &str) -> Result<MemoryWidth, String> {
    match name {
        "i8" => Ok(MemoryWidth::I8),
        "i16" => Ok(MemoryWidth::I16),
        "i32" => Ok(MemoryWidth::I32),
        "i64" => Ok(MemoryWidth::I64),
        "u8" => Ok(MemoryWidth::U8),
        "u16" => Ok(MemoryWidth::U16),
        "u32" => Ok(MemoryWidth::U32),
        "u64" => Ok(MemoryWidth::U64),
        _ => Err(format!(
            "Invalid memory width {name:?}; expected i8, i16, i32, i64, u8, u16, u32, or u64"
        )),
    }
}

fn is_register_name(s: &str) -> bool {
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
