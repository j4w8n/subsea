use crate::ast::{
    Address, AddressOperator, AddressTerm, AssignmentTarget, AssignmentValue, BindingValue,
    CompareOp, Condition, Instruction, Label, MathOp, MemoryDeclaration, MemoryWidth, Operand,
    PrintPart, Program,
};
use crate::grammar::Token;
use std::collections::HashSet;

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
        let mut memory = Vec::new();
        let mut labels = Vec::new();

        while !self.is_at_end() {
            if matches!(self.peek(), Some(Token::Mem)) {
                memory.push(self.parse_memory_declaration()?);
            } else {
                labels.push(self.parse_top_level_label()?);
            }
        }

        validate_memory_names(&memory)?;
        validate_label_storage_names(&memory, &labels)?;
        validate_main_label(&labels)?;

        Ok(Program {
            entry: String::from("main"),
            memory,
            labels,
        })
    }

    fn parse_top_level_label(&mut self) -> Result<Label, String> {
        let name = match self.advance() {
            Some(Token::Ident(name)) => name,
            Some(Token::LocalIdent(name)) => {
                return Err(format!(
                    "Local label .{name} cannot be declared at the top level"
                ));
            }
            Some(token) => return Err(format!("Expected label name, found {token:?}")),
            None => return Err(String::from("Expected label name, found end of input")),
        };

        self.expect(Token::Colon, "Expected ':' after label name")?;

        if !matches!(self.peek(), Some(Token::LBrace)) {
            return Ok(Label {
                name,
                instructions: Vec::new(),
            });
        }

        self.expect(Token::LBrace, "Expected '{' to start label block")?;

        let mut instructions = Vec::new();
        while !matches!(self.peek(), Some(Token::RBrace)) {
            if self.is_at_end() {
                return Err(format!("Expected '}}' to close label '{name}'"));
            }

            instructions.push(self.parse_instruction(&name)?);
        }

        self.expect(Token::RBrace, "Expected '}' after label block")?;

        Ok(Label { name, instructions })
    }

    fn parse_memory_declaration(&mut self) -> Result<MemoryDeclaration, String> {
        self.expect(Token::Mem, "Expected mem declaration")?;

        let name = match self.advance() {
            Some(Token::Ident(name)) => name,
            Some(token) => return Err(format!("Expected memory name after mem, found {token:?}")),
            None => {
                return Err(String::from(
                    "Expected memory name after mem, found end of input",
                ));
            }
        };

        self.expect(Token::Colon, "Expected ':' after memory name")?;

        let width = match self.advance() {
            Some(Token::Ident(name)) => parse_memory_width(&name)?,
            Some(token) => return Err(format!("Expected memory width after ':', found {token:?}")),
            None => {
                return Err(String::from(
                    "Expected memory width after ':', found end of input",
                ));
            }
        };

        match self.peek() {
            Some(Token::Equals) => {
                self.advance();
                let value = self.parse_integer_literal("memory initializer")?;
                validate_integer_binding_width(value, width)?;

                Ok(MemoryDeclaration::Scalar { name, width, value })
            }
            Some(Token::LParen) => {
                self.advance();
                let count = self.parse_buffer_count()?;
                self.expect(Token::RParen, "Expected ')' after buffer count")?;

                Ok(MemoryDeclaration::Buffer { name, width, count })
            }
            Some(token) => Err(format!(
                "Expected '=' for scalar memory or '(' for buffer memory, found {token:?}"
            )),
            None => Err(String::from(
                "Expected '=' for scalar memory or '(' for buffer memory, found end of input",
            )),
        }
    }

    fn parse_instruction(&mut self, current_label: &str) -> Result<Instruction, String> {
        match self.advance() {
            Some(Token::Call) => match self.parse_label_target("call", current_label)? {
                target => Ok(Instruction::Call { target }),
            },
            Some(Token::Jmp) => match self.parse_label_target("jump", current_label)? {
                target => {
                    let condition = self.parse_optional_jump_condition()?;
                    Ok(Instruction::Jmp { target, condition })
                }
            },
            Some(Token::Exit) => {
                let code = self.parse_exit_code()?;
                Ok(Instruction::Exit { code })
            }
            Some(Token::Const) => self.parse_const_declaration(),
            Some(Token::Print) => match self.advance() {
                Some(Token::Ident(name)) => Ok(Instruction::Print {
                    parts: vec![PrintPart::Binding(name)],
                }),
                Some(Token::Register(name)) => Ok(Instruction::Print {
                    parts: vec![PrintPart::Operand(Operand::Register(name))],
                }),
                Some(Token::NumberLiteral(value)) => value
                    .parse::<i64>()
                    .map(|value| Instruction::Print {
                        parts: vec![PrintPart::Operand(Operand::Immediate(value))],
                    })
                    .map_err(|_| format!("Invalid integer literal {value:?}")),
                Some(Token::Minus) => match self.advance() {
                    Some(Token::NumberLiteral(value)) => value
                        .parse::<i64>()
                        .map(|value| Instruction::Print {
                            parts: vec![PrintPart::Operand(Operand::Immediate(-value))],
                        })
                        .map_err(|_| format!("Invalid integer literal -{value}")),
                    Some(token) => Err(format!(
                        "Expected number after '-' in print operand, found {token:?}"
                    )),
                    None => Err(String::from(
                        "Expected number after '-' in print operand, found end of input",
                    )),
                },
                Some(Token::Text(value)) => self.parse_print_literal(value),
                Some(token) => Err(format!(
                    "Expected binding name, register, integer, or string literal after print, found {token:?}"
                )),
                None => Err(String::from(
                    "Expected binding name, register, integer, or string literal after print, found end of input",
                )),
            },
            Some(Token::Pop) => {
                let dst = self.parse_operand()?;
                Ok(Instruction::Pop { dst })
            }
            Some(Token::Push) => {
                let src = self.parse_operand()?;
                Ok(Instruction::Push { src })
            }
            Some(Token::Ret) => Ok(Instruction::Ret),
            Some(Token::Stack) => self.parse_stack_declaration(),
            Some(Token::Syscall) => Ok(Instruction::Syscall),
            Some(Token::Ampersand) => Err(String::from(
                "Address-of syntax is only supported on the right side of assignment",
            )),
            Some(Token::LocalIdent(name)) if matches!(self.peek(), Some(Token::Colon)) => {
                self.advance();
                Ok(Instruction::Label {
                    name: mangle_local_label(current_label, &name),
                })
            }
            Some(Token::Ident(name)) if matches!(self.peek(), Some(Token::Colon)) => Err(format!(
                "Nested label {name}: must be local; write .{name}: instead"
            )),
            Some(Token::Ident(name)) => {
                self.parse_assignment(AssignmentTarget::Operand(Operand::Ident(name)))
            }
            Some(Token::LBracket) => {
                let address = self.parse_address()?;
                self.expect(Token::RBracket, "Expected ']' after memory operand")?;
                let width = self.parse_optional_memory_width()?;

                self.parse_assignment(AssignmentTarget::Operand(Operand::Dereference {
                    address,
                    width,
                }))
            }
            Some(Token::Register(name)) => self.parse_register_assignment(name),
            Some(token) => Err(format!("Expected instruction, found {token:?}")),
            None => Err(String::from("Expected instruction, found end of input")),
        }
    }

    fn parse_const_declaration(&mut self) -> Result<Instruction, String> {
        let name = match self.advance() {
            Some(Token::Ident(name)) => name,
            Some(token) => {
                return Err(format!(
                    "Expected binding name after const, found {token:?}"
                ));
            }
            None => {
                return Err(String::from(
                    "Expected binding name after const, found end of input",
                ));
            }
        };

        let width = self.parse_optional_binding_width()?;
        self.expect(Token::Equals, "Expected '=' after binding name")?;

        let value = self.parse_binding_value(width)?;

        Ok(Instruction::Const { name, value })
    }

    fn parse_stack_declaration(&mut self) -> Result<Instruction, String> {
        let name = match self.advance() {
            Some(Token::Ident(name)) => name,
            Some(token) => {
                return Err(format!(
                    "Expected stack variable name after stack, found {token:?}"
                ));
            }
            None => {
                return Err(String::from(
                    "Expected stack variable name after stack, found end of input",
                ));
            }
        };

        self.expect(Token::Colon, "Expected ':' after stack variable name")?;
        let width = match self.advance() {
            Some(Token::Ident(name)) => parse_memory_width(&name)?,
            Some(token) => {
                return Err(format!(
                    "Expected stack variable width after ':', found {token:?}"
                ));
            }
            None => {
                return Err(String::from(
                    "Expected stack variable width after ':', found end of input",
                ));
            }
        };

        self.expect(Token::Equals, "Expected '=' after stack variable width")?;
        let value = self.parse_operand()?;

        Ok(Instruction::Stack { name, width, value })
    }

    fn parse_label_target(
        &mut self,
        instruction: &str,
        current_label: &str,
    ) -> Result<String, String> {
        match self.advance() {
            Some(Token::Ident(target)) => Ok(target),
            Some(Token::LocalIdent(target)) => Ok(mangle_local_label(current_label, &target)),
            Some(token) => Err(format!(
                "Expected {instruction} target label, found {token:?}"
            )),
            None => Err(format!(
                "Expected {instruction} target label, found end of input"
            )),
        }
    }

    fn parse_optional_jump_condition(&mut self) -> Result<Option<Condition>, String> {
        if !matches!(self.peek(), Some(Token::If)) {
            return Ok(None);
        }

        self.advance();

        let lhs = self.parse_operand()?;
        let op = self.parse_compare_op()?;
        let rhs = self.parse_operand()?;

        Ok(Some(Condition { lhs, op, rhs }))
    }

    fn parse_compare_op(&mut self) -> Result<CompareOp, String> {
        match self.advance() {
            Some(Token::EqualsEquals) => Ok(CompareOp::Equal),
            Some(Token::NotEquals) => Ok(CompareOp::NotEqual),
            Some(Token::ILess) => Ok(CompareOp::SignedLess),
            Some(Token::ILessEquals) => Ok(CompareOp::SignedLessEqual),
            Some(Token::IGreater) => Ok(CompareOp::SignedGreater),
            Some(Token::IGreaterEquals) => Ok(CompareOp::SignedGreaterEqual),
            Some(Token::ULess) => Ok(CompareOp::UnsignedLess),
            Some(Token::ULessEquals) => Ok(CompareOp::UnsignedLessEqual),
            Some(Token::UGreater) => Ok(CompareOp::UnsignedGreater),
            Some(Token::UGreaterEquals) => Ok(CompareOp::UnsignedGreaterEqual),
            Some(Token::Less) => Err(String::from(
                "Comparison '<' must specify signedness; use i< or u<",
            )),
            Some(Token::LessEquals) => Err(String::from(
                "Comparison '<=' must specify signedness; use i<= or u<=",
            )),
            Some(Token::Greater) => Err(String::from(
                "Comparison '>' must specify signedness; use i> or u>",
            )),
            Some(Token::GreaterEquals) => Err(String::from(
                "Comparison '>=' must specify signedness; use i>= or u>=",
            )),
            Some(token) => Err(format!("Expected comparison operator, found {token:?}")),
            None => Err(String::from(
                "Expected comparison operator, found end of input",
            )),
        }
    }

    fn parse_register_assignment(&mut self, high_or_dst: String) -> Result<Instruction, String> {
        if !matches!(self.peek(), Some(Token::Colon)) {
            return self
                .parse_assignment(AssignmentTarget::Operand(Operand::Register(high_or_dst)));
        }

        self.advance();
        let low = match self.advance() {
            Some(Token::Register(name)) => name,
            Some(token) => {
                return Err(format!(
                    "Expected low register after register-pair ':', found {token:?}"
                ));
            }
            None => {
                return Err(String::from(
                    "Expected low register after register-pair ':', found end of input",
                ));
            }
        };

        self.parse_assignment(AssignmentTarget::RegisterPair {
            high: high_or_dst,
            low,
        })
    }

    fn parse_assignment(&mut self, dst: AssignmentTarget) -> Result<Instruction, String> {
        self.expect(Token::Equals, "Expected '=' after assignment destination")?;

        let lhs = self.parse_operand()?;
        let value = match self.peek() {
            Some(
                Token::ISlash
                | Token::IStar
                | Token::Plus
                | Token::Minus
                | Token::Slash
                | Token::Star
                | Token::USlash
                | Token::UStar,
            ) => match self.advance() {
                Some(Token::Plus) => {
                    let rhs = self.parse_operand()?;
                    AssignmentValue::Binary {
                        op: MathOp::Add,
                        lhs,
                        rhs,
                    }
                }
                Some(Token::Minus) => {
                    let rhs = self.parse_operand()?;
                    AssignmentValue::Binary {
                        op: MathOp::Subtract,
                        lhs,
                        rhs,
                    }
                }
                Some(Token::Star) => {
                    let rhs = self.parse_operand()?;
                    AssignmentValue::Binary {
                        op: MathOp::Multiply,
                        lhs,
                        rhs,
                    }
                }
                Some(Token::Slash) => {
                    return Err(String::from(
                        "Use rdx:rax = lhs u/ rhs or rdx:rax = lhs i/ rhs for division",
                    ));
                }
                Some(operator @ (Token::ISlash | Token::IStar | Token::USlash | Token::UStar)) => {
                    let (is_division, signed) = match operator {
                        Token::ISlash => (true, true),
                        Token::IStar => (false, true),
                        Token::USlash => (true, false),
                        Token::UStar => (false, false),
                        _ => unreachable!(),
                    };
                    let rhs = self.parse_operand()?;

                    if is_division {
                        AssignmentValue::WideDivide { signed, lhs, rhs }
                    } else {
                        AssignmentValue::WideMultiply { signed, lhs, rhs }
                    }
                }
                _ => unreachable!(),
            },
            _ => AssignmentValue::Operand(lhs),
        };

        Ok(Instruction::Assign { dst, value })
    }

    fn parse_print_literal(&mut self, value: String) -> Result<Instruction, String> {
        if !matches!(self.peek(), Some(Token::Comma)) {
            return Ok(Instruction::Print {
                parts: vec![PrintPart::Literal(value)],
            });
        }

        let mut args = Vec::new();
        while matches!(self.peek(), Some(Token::Comma)) {
            self.advance();

            match self.advance() {
                Some(Token::Ident(name)) => args.push(name),
                Some(token) => {
                    return Err(format!(
                        "Expected binding name after print format comma, found {token:?}"
                    ));
                }
                None => {
                    return Err(String::from(
                        "Expected binding name after print format comma, found end of input",
                    ));
                }
            }
        }

        let literal_parts = split_format_literal(&value)?;
        if literal_parts.len() != args.len() + 1 {
            return Err(format!(
                "Print format expected {} argument(s), found {}",
                literal_parts.len().saturating_sub(1),
                args.len()
            ));
        }

        let mut parts = Vec::new();
        for (index, literal) in literal_parts.into_iter().enumerate() {
            if !literal.is_empty() {
                parts.push(PrintPart::Literal(literal));
            }

            if let Some(arg) = args.get(index) {
                parts.push(PrintPart::Binding(arg.clone()));
            }
        }

        Ok(Instruction::Print { parts })
    }

    fn parse_optional_binding_width(&mut self) -> Result<Option<MemoryWidth>, String> {
        if !matches!(self.peek(), Some(Token::Colon)) {
            return Ok(None);
        }

        self.advance();

        match self.advance() {
            Some(Token::Ident(name)) => parse_memory_width(&name).map(Some),
            Some(token) => Err(format!("Expected binding width after ':', found {token:?}")),
            None => Err(String::from(
                "Expected binding width after ':', found end of input",
            )),
        }
    }

    fn parse_binding_value(&mut self, width: Option<MemoryWidth>) -> Result<BindingValue, String> {
        match self.advance() {
            Some(Token::Text(value)) => {
                if width.is_some() {
                    return Err(String::from("String bindings cannot have an integer width"));
                }

                Ok(BindingValue::String(value))
            }
            Some(Token::NumberLiteral(value)) => parse_integer_binding_value(&value, width),
            Some(Token::Minus) => match self.advance() {
                Some(Token::NumberLiteral(value)) => {
                    parse_integer_binding_value(&format!("-{value}"), width)
                }
                Some(token) => Err(format!(
                    "Expected number after '-' in binding value, found {token:?}"
                )),
                None => Err(String::from(
                    "Expected number after '-' in binding value, found end of input",
                )),
            },
            Some(token) => Err(format!(
                "Expected string or integer literal after '=', found {token:?}"
            )),
            None => Err(String::from(
                "Expected string or integer literal after '=', found end of input",
            )),
        }
    }

    fn parse_integer_literal(&mut self, context: &str) -> Result<i64, String> {
        match self.advance() {
            Some(Token::NumberLiteral(value)) => value
                .parse::<i64>()
                .map_err(|_| format!("Invalid integer {context} {value:?}")),
            Some(Token::Minus) => match self.advance() {
                Some(Token::NumberLiteral(value)) => value
                    .parse::<i64>()
                    .map(|value| -value)
                    .map_err(|_| format!("Invalid integer {context} -{value}")),
                Some(token) => Err(format!(
                    "Expected number after '-' in {context}, found {token:?}"
                )),
                None => Err(format!(
                    "Expected number after '-' in {context}, found end of input"
                )),
            },
            Some(token) => Err(format!("Expected integer {context}, found {token:?}")),
            None => Err(format!("Expected integer {context}, found end of input")),
        }
    }

    fn parse_buffer_count(&mut self) -> Result<usize, String> {
        let value = match self.advance() {
            Some(Token::NumberLiteral(value)) => value
                .parse::<usize>()
                .map_err(|_| format!("Invalid buffer count {value:?}"))?,
            Some(Token::Minus) => return Err(String::from("Buffer count must be greater than 0")),
            Some(token) => return Err(format!("Expected buffer count, found {token:?}")),
            None => return Err(String::from("Expected buffer count, found end of input")),
        };

        if value == 0 {
            Err(String::from("Buffer count must be greater than 0"))
        } else {
            Ok(value)
        }
    }

    fn parse_exit_code(&mut self) -> Result<u8, String> {
        let code = match self.advance() {
            Some(Token::NumberLiteral(value)) => value
                .parse::<u16>()
                .map_err(|_| format!("Invalid exit code {value:?}"))?,
            Some(Token::Minus) => return Err(String::from("Exit code must be between 0 and 255")),
            Some(token) => return Err(format!("Expected exit code, found {token:?}")),
            None => return Err(String::from("Expected exit code, found end of input")),
        };

        if code <= 255 {
            Ok(code as u8)
        } else {
            Err(String::from("Exit code must be between 0 and 255"))
        }
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

fn mangle_local_label(parent: &str, name: &str) -> String {
    format!(".L.{parent}.{name}")
}

fn parse_integer_binding_value(
    value: &str,
    width: Option<MemoryWidth>,
) -> Result<BindingValue, String> {
    let value = value
        .parse::<i64>()
        .map_err(|_| format!("Invalid integer binding value {value:?}"))?;

    if let Some(width) = width {
        validate_integer_binding_width(value, width)?;
    }

    Ok(BindingValue::Integer { value, width })
}

fn validate_integer_binding_width(value: i64, width: MemoryWidth) -> Result<(), String> {
    let valid = match width {
        MemoryWidth::I8 => i8::MIN as i64 <= value && value <= i8::MAX as i64,
        MemoryWidth::I16 => i16::MIN as i64 <= value && value <= i16::MAX as i64,
        MemoryWidth::I32 => i32::MIN as i64 <= value && value <= i32::MAX as i64,
        MemoryWidth::I64 => true,
        MemoryWidth::U8 => 0 <= value && value <= u8::MAX as i64,
        MemoryWidth::U16 => 0 <= value && value <= u16::MAX as i64,
        MemoryWidth::U32 => 0 <= value && value <= u32::MAX as i64,
        MemoryWidth::U64 => 0 <= value,
    };

    if valid {
        Ok(())
    } else {
        Err(format!(
            "Integer binding value {value} does not fit in {}",
            memory_width_name(width)
        ))
    }
}

fn memory_width_name(width: MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::I8 => "i8",
        MemoryWidth::I16 => "i16",
        MemoryWidth::I32 => "i32",
        MemoryWidth::I64 => "i64",
        MemoryWidth::U8 => "u8",
        MemoryWidth::U16 => "u16",
        MemoryWidth::U32 => "u32",
        MemoryWidth::U64 => "u64",
    }
}

fn validate_memory_names(memory: &[MemoryDeclaration]) -> Result<(), String> {
    let mut names = HashSet::new();

    for declaration in memory {
        let name = match declaration {
            MemoryDeclaration::Scalar { name, .. } | MemoryDeclaration::Buffer { name, .. } => name,
        };

        if !names.insert(name) {
            return Err(format!("Memory name {name:?} is already defined"));
        }
    }

    Ok(())
}

fn validate_label_storage_names(
    memory: &[MemoryDeclaration],
    labels: &[Label],
) -> Result<(), String> {
    let memory_names: HashSet<_> = memory
        .iter()
        .map(|declaration| match declaration {
            MemoryDeclaration::Scalar { name, .. } | MemoryDeclaration::Buffer { name, .. } => name,
        })
        .collect();

    for label in labels {
        let mut names = HashSet::new();

        for instruction in &label.instructions {
            match instruction {
                Instruction::Const { name, .. } | Instruction::Stack { name, .. } => {
                    if memory_names.contains(name) {
                        return Err(format!(
                            "Name {name:?} in label {:?} conflicts with top-level memory",
                            label.name
                        ));
                    }

                    if !names.insert(name) {
                        return Err(format!(
                            "Name {name:?} is already defined in label {:?}",
                            label.name
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn validate_main_label(labels: &[Label]) -> Result<(), String> {
    if labels.iter().any(|label| label.name == "main") {
        Ok(())
    } else {
        Err(String::from("Program must define a top-level main label"))
    }
}

fn split_format_literal(value: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars().peekable();

    while let Some(char) = chars.next() {
        match char {
            '{' => match chars.peek() {
                Some('}') => {
                    chars.next();
                    parts.push(current);
                    current = String::new();
                }
                _ => {
                    return Err(String::from(
                        "Only '{}' print format placeholders are supported",
                    ));
                }
            },
            '}' => return Err(String::from("Unmatched '}' in print format string")),
            _ => current.push(char),
        }
    }

    parts.push(current);

    Ok(parts)
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
