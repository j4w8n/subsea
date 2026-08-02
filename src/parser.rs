use crate::ast::{Instruction, Label, Operand, Program};
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
            Some(Token::Copy) => {
                let src = self.parse_operand()?;
                self.expect(Token::Comma, "Expected ',' after copy source operand")?;
                let dst = self.parse_operand()?;

                Ok(Instruction::Copy { src, dst })
            }
            Some(Token::Syscall) => Ok(Instruction::Syscall),
            Some(token) => Err(format!("Expected instruction, found {token:?}")),
            None => Err(String::from("Expected instruction, found end of input")),
        }
    }

    fn parse_operand(&mut self) -> Result<Operand, String> {
        match self.advance() {
            Some(Token::NumberLiteral(value)) => value
                .parse::<i64>()
                .map(Operand::Immediate)
                .map_err(|_| format!("Invalid integer literal {value:?}")),
            Some(Token::Register(name)) => Ok(Operand::Register(name)),
            Some(Token::Ident(name)) => Ok(Operand::Ident(name)),
            Some(Token::Pointer(name)) => Ok(Operand::Pointer(name)),
            Some(token) => Err(format!("Expected operand, found {token:?}")),
            None => Err(String::from("Expected operand, found end of input")),
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
