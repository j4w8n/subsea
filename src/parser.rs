use crate::ast::{
    Address, AddressOperator, AddressTerm, AssignmentTarget, AssignmentValue, BindingValue,
    CompareOp, Condition, ConditionExpr, FloatMathOp, Instruction, Label, MathOp,
    MemoryDeclaration, MemoryWidth, Operand, PrintPart, Program, ReadSource, StringInitializer,
    StringProperty,
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

        let name = self.expect_ident("memory name after mem")?;

        self.expect(Token::Colon, "Expected ':' after memory name")?;

        let width = match self.advance() {
            Some(Token::Ident(name)) => MemoryWidth::parse(&name)?,
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
                if width.is_float() {
                    let value = self.parse_float_literal("memory initializer", width)?;

                    return Ok(MemoryDeclaration::FloatScalar { name, width, value });
                }

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
            Some(Token::Call) => {
                let target = self.parse_label_target("call", current_label)?;
                Ok(Instruction::Call { target })
            }
            Some(Token::Jmp) => {
                let target = self.parse_label_target("jump", current_label)?;
                let condition = self.parse_optional_jump_condition()?;
                Ok(Instruction::Jmp { target, condition })
            }
            Some(Token::Exit) => {
                let code = self.parse_exit_code()?;
                Ok(Instruction::Exit { code })
            }
            Some(Token::Const) => self.parse_const_declaration(),
            Some(Token::Print) => match self.advance() {
                Some(Token::Ident(name)) => {
                    if matches!(self.peek(), Some(Token::LocalIdent(_))) {
                        return self
                            .parse_ident_operand(name)
                            .map(|operand| Instruction::Print {
                                parts: vec![PrintPart::Operand(operand)],
                            });
                    }

                    Ok(Instruction::Print {
                        parts: vec![PrintPart::Binding(name)],
                    })
                }
                Some(Token::Register(name)) => Ok(Instruction::Print {
                    parts: vec![PrintPart::Operand(Operand::Register(name))],
                }),
                Some(Token::NumberLiteral(value)) => value
                    .parse::<i128>()
                    .map(|value| Instruction::Print {
                        parts: vec![PrintPart::Operand(Operand::Immediate(value))],
                    })
                    .map_err(|_| format!("Invalid integer literal {value:?}")),
                Some(Token::FloatLiteral(value)) => Err(format!(
                    "Float literal {value} cannot be printed directly yet; bind it with const name:f32 or const name:f64 first"
                )),
                Some(Token::Minus) => match self.advance() {
                    Some(Token::NumberLiteral(value)) => parse_signed_integer(&value, true)
                        .map_err(|_| format!("Invalid integer literal -{value}"))
                        .map(|value| Instruction::Print {
                            parts: vec![PrintPart::Operand(Operand::Immediate(value))],
                        }),
                    Some(Token::FloatLiteral(value)) => Err(format!(
                        "Float literal -{value} cannot be printed directly yet; bind it with const name:f32 or const name:f64 first"
                    )),
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
            Some(Token::Read) => self.parse_read_instruction(),
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
        let name = self.expect_ident("binding name after const")?;

        let width = self.parse_optional_binding_width()?;
        self.expect(Token::Equals, "Expected '=' after binding name")?;

        let value = self.parse_binding_value(width)?;

        Ok(Instruction::Const { name, value })
    }

    fn parse_stack_declaration(&mut self) -> Result<Instruction, String> {
        let name = self.expect_ident("stack variable name after stack")?;

        self.expect(Token::Colon, "Expected ':' after stack variable name")?;
        let width_name = self.expect_ident("stack variable type after ':'")?;

        if width_name == "str" {
            self.expect(Token::Equals, "Expected '=' after stack string type")?;
            let value = self.parse_string_initializer()?;

            return Ok(Instruction::StackString { name, value });
        }

        let width = MemoryWidth::parse(&width_name)?;

        self.expect(Token::Equals, "Expected '=' after stack variable width")?;
        let value = self.parse_operand()?;

        Ok(Instruction::Stack { name, width, value })
    }

    fn parse_read_instruction(&mut self) -> Result<Instruction, String> {
        let src = match self.advance() {
            Some(Token::Stdin) => ReadSource::Stdin,
            Some(token) => {
                return Err(format!("Expected read source stdin, found {token:?}"));
            }
            None => {
                return Err(String::from(
                    "Expected read source stdin, found end of input",
                ));
            }
        };

        self.expect(Token::Comma, "Expected ',' after read source")?;
        let dst = self.parse_operand()?;
        self.expect(Token::Comma, "Expected ',' after read destination")?;
        let len = self.parse_operand()?;

        Ok(Instruction::Read { src, dst, len })
    }

    fn parse_string_initializer(&mut self) -> Result<StringInitializer, String> {
        match self.advance() {
            Some(Token::Text(value)) => Ok(StringInitializer::Literal(value)),
            Some(Token::Slice) => {
                let ptr = self.parse_operand()?;
                self.expect(Token::Comma, "Expected ',' after slice pointer")?;
                let len = self.parse_operand()?;

                Ok(StringInitializer::Slice { ptr, len })
            }
            Some(Token::Ident(name)) => Err(format!(
                "Expected string literal or slice initializer after '=', found {name:?}"
            )),
            Some(token) => Err(format!(
                "Expected string literal or slice initializer after '=', found {token:?}"
            )),
            None => Err(String::from(
                "Expected string literal or slice initializer after '=', found end of input",
            )),
        }
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

    fn parse_optional_jump_condition(&mut self) -> Result<Option<ConditionExpr>, String> {
        if !matches!(self.peek(), Some(Token::If)) {
            return Ok(None);
        }

        self.advance();
        self.parse_condition_expr().map(Some)
    }

    fn parse_optional_assignment_condition(&mut self) -> Result<Option<ConditionExpr>, String> {
        if !matches!(self.peek(), Some(Token::If)) {
            return Ok(None);
        }

        self.advance();
        self.parse_condition_expr().map(Some)
    }

    fn parse_condition_expr(&mut self) -> Result<ConditionExpr, String> {
        let lhs = self.parse_operand()?;
        self.parse_condition_expr_after_lhs(lhs)
    }

    fn parse_condition_expr_after_lhs(&mut self, lhs: Operand) -> Result<ConditionExpr, String> {
        if matches!(self.peek(), Some(Token::Ampersand)) {
            self.advance();
            let rhs = self.parse_operand()?;
            let op = self.parse_compare_op()?;
            let zero = self.parse_operand()?;

            if !matches!(op, CompareOp::Equal | CompareOp::NotEqual) {
                return Err(String::from(
                    "Bitwise-and conditions only support == 0 or != 0",
                ));
            }

            if zero != Operand::Immediate(0) {
                return Err(String::from(
                    "Bitwise-and conditions must compare against 0",
                ));
            }

            return Ok(ConditionExpr::BitwiseAndZero { lhs, rhs, op });
        }

        let op = self.parse_compare_op()?;
        let rhs = self.parse_operand()?;

        Ok(ConditionExpr::Compare(Condition { lhs, op, rhs }))
    }

    fn parse_compare_op(&mut self) -> Result<CompareOp, String> {
        match self.advance() {
            Some(token) if compare_op(&token).is_some() => Ok(compare_op(&token).unwrap()),
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
        let low = self.expect_register("low register after register-pair ':'")?;

        self.parse_assignment(AssignmentTarget::RegisterPair {
            high: high_or_dst,
            low,
        })
    }

    fn parse_assignment(&mut self, dst: AssignmentTarget) -> Result<Instruction, String> {
        self.expect(Token::Equals, "Expected '=' after assignment destination")?;

        let value = if matches!(self.peek(), Some(Token::Tilde)) {
            self.advance();
            let operand = self.parse_operand()?;
            AssignmentValue::BitwiseUnary {
                op: crate::ast::BitwiseUnaryOp::Not,
                operand,
            }
        } else {
            let lhs = self.parse_operand()?;
            match self.peek().and_then(assignment_op) {
                Some(AssignmentOp::UnsupportedDivision) => {
                    self.advance();
                    return Err(String::from(
                        "Use rdx:rax = lhs u/ rhs or rdx:rax = lhs i/ rhs for division",
                    ));
                }
                Some(AssignmentOp::Binary(MathOp::BitAnd)) => {
                    self.advance();
                    let rhs = self.parse_operand()?;

                    if self.peek().and_then(compare_op).is_some() {
                        let op = self.parse_compare_op()?;
                        let zero = self.parse_operand()?;

                        if !matches!(op, CompareOp::Equal | CompareOp::NotEqual) {
                            return Err(String::from(
                                "Bitwise-and conditions only support == 0 or != 0",
                            ));
                        }

                        if zero != Operand::Immediate(0) {
                            return Err(String::from(
                                "Bitwise-and conditions must compare against 0",
                            ));
                        }

                        AssignmentValue::Condition(ConditionExpr::BitwiseAndZero { lhs, rhs, op })
                    } else {
                        AssignmentValue::Binary {
                            op: MathOp::BitAnd,
                            lhs,
                            rhs,
                        }
                    }
                }
                Some(op) => {
                    self.advance();
                    let rhs = self.parse_operand()?;
                    assignment_value(op, lhs, rhs)
                }
                _ if self.peek().and_then(compare_op).is_some() => {
                    AssignmentValue::Condition(self.parse_condition_expr_after_lhs(lhs)?)
                }
                _ => AssignmentValue::Operand(lhs),
            }
        };

        if let Some(condition) = self.parse_optional_assignment_condition()? {
            Ok(Instruction::AssignIf {
                dst,
                value,
                condition,
            })
        } else {
            Ok(Instruction::Assign { dst, value })
        }
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
        self.parse_optional_width("binding width")
    }

    fn parse_binding_value(&mut self, width: Option<MemoryWidth>) -> Result<BindingValue, String> {
        match self.advance() {
            Some(Token::Text(value)) => {
                if width.is_some() {
                    return Err(String::from("String bindings cannot have an integer width"));
                }

                Ok(BindingValue::String(value))
            }
            Some(Token::NumberLiteral(value)) if width.is_some_and(MemoryWidth::is_float) => {
                parse_float_binding_value(&value, width)
            }
            Some(Token::NumberLiteral(value)) => parse_integer_binding_value(&value, width),
            Some(Token::FloatLiteral(value)) => parse_float_binding_value(&value, width),
            Some(Token::Minus) => match self.advance() {
                Some(Token::NumberLiteral(value)) if width.is_some_and(MemoryWidth::is_float) => {
                    parse_float_binding_value(&format!("-{value}"), width)
                }
                Some(Token::NumberLiteral(value)) => {
                    let value = parse_signed_integer(&value, true)
                        .map_err(|_| format!("Invalid integer binding value -{value}"))?;
                    parse_integer_binding_value(&value.to_string(), width)
                }
                Some(Token::FloatLiteral(value)) => {
                    parse_float_binding_value(&format!("-{value}"), width)
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

    fn parse_integer_literal(&mut self, context: &str) -> Result<i128, String> {
        match self.advance() {
            Some(Token::NumberLiteral(value)) => value
                .parse::<i128>()
                .map_err(|_| format!("Invalid integer {context} {value:?}")),
            Some(Token::Minus) => match self.advance() {
                Some(Token::NumberLiteral(value)) => parse_signed_integer(&value, true)
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

    fn parse_float_literal(&mut self, context: &str, width: MemoryWidth) -> Result<String, String> {
        match self.advance() {
            Some(Token::NumberLiteral(value) | Token::FloatLiteral(value)) => {
                validate_float_literal(&value, width, context)?;
                Ok(value)
            }
            Some(Token::Minus) => match self.advance() {
                Some(Token::NumberLiteral(value) | Token::FloatLiteral(value)) => {
                    let value = format!("-{value}");
                    validate_float_literal(&value, width, context)?;
                    Ok(value)
                }
                Some(token) => Err(format!(
                    "Expected number after '-' in {context}, found {token:?}"
                )),
                None => Err(format!(
                    "Expected number after '-' in {context}, found end of input"
                )),
            },
            Some(token) => Err(format!("Expected float {context}, found {token:?}")),
            None => Err(format!("Expected float {context}, found end of input")),
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
                Some(Token::FloatLiteral(value)) => Err(format!(
                    "Cannot take the address of float literal {value}; expected a label after '&'"
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
                Some(Token::NumberLiteral(value)) => parse_signed_integer(&value, true)
                    .map(Operand::Immediate)
                    .map_err(|_| format!("Invalid integer literal -{value}")),
                Some(Token::FloatLiteral(value)) => Ok(Operand::FloatLiteral(format!("-{value}"))),
                Some(token) => Err(format!("Expected number after '-', found {token:?}")),
                None => Err(String::from(
                    "Expected number after '-', found end of input",
                )),
            },
            Some(Token::NumberLiteral(value)) => value
                .parse::<i128>()
                .map(Operand::Immediate)
                .map_err(|_| format!("Invalid integer literal {value:?}")),
            Some(Token::FloatLiteral(value)) => Ok(Operand::FloatLiteral(value)),
            Some(Token::Register(name)) => Ok(Operand::Register(name)),
            Some(Token::Ident(name)) => self.parse_ident_operand(name),
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
        self.parse_optional_width("memory width")
    }

    fn parse_optional_width(&mut self, expected: &str) -> Result<Option<MemoryWidth>, String> {
        if !matches!(self.peek(), Some(Token::Colon)) {
            return Ok(None);
        }

        self.advance();

        match self.advance() {
            Some(Token::Ident(name)) => MemoryWidth::parse(&name).map(Some),
            Some(token) => Err(format!("Expected {expected} after ':', found {token:?}")),
            None => Err(format!("Expected {expected} after ':', found end of input")),
        }
    }

    fn parse_ident_operand(&mut self, name: String) -> Result<Operand, String> {
        let Some(Token::LocalIdent(property)) = self.peek().cloned() else {
            return Ok(Operand::Ident(name));
        };

        self.advance();

        let property = match property.as_str() {
            "len" => StringProperty::Len,
            "ptr" => StringProperty::Ptr,
            _ => {
                return Err(format!(
                    "Unknown property .{property}; expected .ptr or .len"
                ));
            }
        };

        Ok(Operand::StringProperty { name, property })
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
                    .parse::<i128>()
                    .map(AddressTerm::Immediate)
                    .map_err(|_| format!("Invalid integer literal {value:?}"))
            }
            Some(Token::FloatLiteral(value)) => Err(format!(
                "Float literal {value} is not valid inside a memory operand"
            )),
            Some(Token::Register(name)) => {
                if is_xmm_register_name(&name) {
                    return Err(format!(
                        "XMM register {name} cannot be used as a memory address"
                    ));
                }

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

    fn expect_ident(&mut self, expected: &str) -> Result<String, String> {
        match self.advance() {
            Some(Token::Ident(name)) => Ok(name),
            Some(token) => Err(format!("Expected {expected}, found {token:?}")),
            None => Err(format!("Expected {expected}, found end of input")),
        }
    }

    fn expect_register(&mut self, expected: &str) -> Result<String, String> {
        match self.advance() {
            Some(Token::Register(name)) => Ok(name),
            Some(token) => Err(format!("Expected {expected}, found {token:?}")),
            None => Err(format!("Expected {expected}, found end of input")),
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

fn mangle_local_label(parent: &str, name: &str) -> String {
    format!(".L.{parent}.{name}")
}

fn compare_op(token: &Token) -> Option<CompareOp> {
    Some(match token {
        Token::EqualsEquals => CompareOp::Equal,
        Token::NotEquals => CompareOp::NotEqual,
        Token::F32EqualsEquals => CompareOp::FloatEqual(MemoryWidth::F32),
        Token::F32NotEquals => CompareOp::FloatNotEqual(MemoryWidth::F32),
        Token::F32Less => CompareOp::FloatLess(MemoryWidth::F32),
        Token::F32LessEquals => CompareOp::FloatLessEqual(MemoryWidth::F32),
        Token::F32Greater => CompareOp::FloatGreater(MemoryWidth::F32),
        Token::F32GreaterEquals => CompareOp::FloatGreaterEqual(MemoryWidth::F32),
        Token::F64EqualsEquals => CompareOp::FloatEqual(MemoryWidth::F64),
        Token::F64NotEquals => CompareOp::FloatNotEqual(MemoryWidth::F64),
        Token::F64Less => CompareOp::FloatLess(MemoryWidth::F64),
        Token::F64LessEquals => CompareOp::FloatLessEqual(MemoryWidth::F64),
        Token::F64Greater => CompareOp::FloatGreater(MemoryWidth::F64),
        Token::F64GreaterEquals => CompareOp::FloatGreaterEqual(MemoryWidth::F64),
        Token::ILess => CompareOp::SignedLess,
        Token::ILessEquals => CompareOp::SignedLessEqual,
        Token::IGreater => CompareOp::SignedGreater,
        Token::IGreaterEquals => CompareOp::SignedGreaterEqual,
        Token::ULess => CompareOp::UnsignedLess,
        Token::ULessEquals => CompareOp::UnsignedLessEqual,
        Token::UGreater => CompareOp::UnsignedGreater,
        Token::UGreaterEquals => CompareOp::UnsignedGreaterEqual,
        Token::Less => CompareOp::Less,
        Token::LessEquals => CompareOp::LessEqual,
        Token::Greater => CompareOp::Greater,
        Token::GreaterEquals => CompareOp::GreaterEqual,
        _ => return None,
    })
}

#[derive(Clone, Copy)]
enum AssignmentOp {
    Binary(MathOp),
    FloatBinary(MemoryWidth, FloatMathOp),
    UnsupportedDivision,
    Wide { signed: bool, division: bool },
}

fn assignment_op(token: &Token) -> Option<AssignmentOp> {
    Some(match token {
        Token::Plus => AssignmentOp::Binary(MathOp::Add),
        Token::Ampersand => AssignmentOp::Binary(MathOp::BitAnd),
        Token::Pipe => AssignmentOp::Binary(MathOp::BitOr),
        Token::Caret => AssignmentOp::Binary(MathOp::BitXor),
        Token::ShiftLeft => AssignmentOp::Binary(MathOp::ShiftLeft),
        Token::ShiftRight => AssignmentOp::Binary(MathOp::ShiftRightLogical),
        Token::IShiftRight => AssignmentOp::Binary(MathOp::ShiftRightArithmetic),
        Token::Minus => AssignmentOp::Binary(MathOp::Subtract),
        Token::Star => AssignmentOp::Binary(MathOp::Multiply),
        Token::Slash => AssignmentOp::UnsupportedDivision,
        Token::F32Plus => AssignmentOp::FloatBinary(MemoryWidth::F32, FloatMathOp::Add),
        Token::F32Minus => AssignmentOp::FloatBinary(MemoryWidth::F32, FloatMathOp::Subtract),
        Token::F32Star => AssignmentOp::FloatBinary(MemoryWidth::F32, FloatMathOp::Multiply),
        Token::F32Slash => AssignmentOp::FloatBinary(MemoryWidth::F32, FloatMathOp::Divide),
        Token::F64Plus => AssignmentOp::FloatBinary(MemoryWidth::F64, FloatMathOp::Add),
        Token::F64Minus => AssignmentOp::FloatBinary(MemoryWidth::F64, FloatMathOp::Subtract),
        Token::F64Star => AssignmentOp::FloatBinary(MemoryWidth::F64, FloatMathOp::Multiply),
        Token::F64Slash => AssignmentOp::FloatBinary(MemoryWidth::F64, FloatMathOp::Divide),
        Token::ISlash => AssignmentOp::Wide {
            signed: true,
            division: true,
        },
        Token::IStar => AssignmentOp::Wide {
            signed: true,
            division: false,
        },
        Token::USlash => AssignmentOp::Wide {
            signed: false,
            division: true,
        },
        Token::UStar => AssignmentOp::Wide {
            signed: false,
            division: false,
        },
        _ => return None,
    })
}

fn assignment_value(op: AssignmentOp, lhs: Operand, rhs: Operand) -> AssignmentValue {
    match op {
        AssignmentOp::Binary(op) => AssignmentValue::Binary { op, lhs, rhs },
        AssignmentOp::FloatBinary(width, op) => AssignmentValue::FloatBinary {
            width,
            op,
            lhs,
            rhs,
        },
        AssignmentOp::Wide { signed, division } => {
            if division {
                AssignmentValue::WideDivide { signed, lhs, rhs }
            } else {
                AssignmentValue::WideMultiply { signed, lhs, rhs }
            }
        }
        AssignmentOp::UnsupportedDivision => unreachable!(),
    }
}

fn parse_integer_binding_value(
    value: &str,
    width: Option<MemoryWidth>,
) -> Result<BindingValue, String> {
    if width.is_some_and(MemoryWidth::is_float) {
        return Err(format!(
            "Integer binding value {value:?} cannot use floating-point width"
        ));
    }

    let value = value
        .parse::<i128>()
        .map_err(|_| format!("Invalid integer binding value {value:?}"))?;

    if let Some(width) = width {
        validate_integer_binding_width(value, width)?;
    }

    Ok(BindingValue::Integer { value, width })
}

fn parse_float_binding_value(
    value: &str,
    width: Option<MemoryWidth>,
) -> Result<BindingValue, String> {
    let width = width.ok_or_else(|| {
        format!("Float binding value {value:?} requires an explicit f32 or f64 width")
    })?;

    if !width.is_float() {
        return Err(format!(
            "Float binding value {value:?} requires f32 or f64 width"
        ));
    }

    validate_float_literal(value, width, "binding value")?;

    Ok(BindingValue::Float {
        value: value.to_string(),
        width,
    })
}

fn validate_float_literal(value: &str, width: MemoryWidth, context: &str) -> Result<(), String> {
    let valid = match width {
        MemoryWidth::F32 => value.parse::<f32>().is_ok_and(f32::is_finite),
        MemoryWidth::F64 => value.parse::<f64>().is_ok_and(f64::is_finite),
        _ => return Err(format!("Float {context} requires f32 or f64 width")),
    };

    if valid {
        Ok(())
    } else {
        Err(format!("Invalid float {context} {value:?}"))
    }
}

fn parse_signed_integer(value: &str, negative: bool) -> Result<i128, ()> {
    if negative {
        value.parse::<i128>().map(|value| -value).map_err(|_| ())
    } else {
        value.parse::<i128>().map_err(|_| ())
    }
}

fn validate_integer_binding_width(value: i128, width: MemoryWidth) -> Result<(), String> {
    let valid = match width {
        MemoryWidth::F32 | MemoryWidth::F64 => {
            return Err(format!(
                "Integer binding value {value} cannot use {}",
                width.name()
            ));
        }
        MemoryWidth::I8 => i8::MIN as i128 <= value && value <= i8::MAX as i128,
        MemoryWidth::I16 => i16::MIN as i128 <= value && value <= i16::MAX as i128,
        MemoryWidth::I32 => i32::MIN as i128 <= value && value <= i32::MAX as i128,
        MemoryWidth::I64 => i64::MIN as i128 <= value && value <= i64::MAX as i128,
        MemoryWidth::U8 => 0 <= value && value <= u8::MAX as i128,
        MemoryWidth::U16 => 0 <= value && value <= u16::MAX as i128,
        MemoryWidth::U32 => 0 <= value && value <= u32::MAX as i128,
        MemoryWidth::U64 => 0 <= value && value <= u64::MAX as i128,
    };

    if valid {
        Ok(())
    } else {
        Err(format!(
            "Integer binding value {value} does not fit in {}",
            width.name()
        ))
    }
}

fn validate_memory_names(memory: &[MemoryDeclaration]) -> Result<(), String> {
    let mut names = HashSet::new();

    for declaration in memory {
        let name = match declaration {
            MemoryDeclaration::Scalar { name, .. }
            | MemoryDeclaration::FloatScalar { name, .. }
            | MemoryDeclaration::Buffer { name, .. } => name,
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
            MemoryDeclaration::Scalar { name, .. }
            | MemoryDeclaration::FloatScalar { name, .. }
            | MemoryDeclaration::Buffer { name, .. } => name,
        })
        .collect();

    for label in labels {
        let mut names = HashSet::new();

        for instruction in &label.instructions {
            match instruction {
                Instruction::Const { name, .. }
                | Instruction::Stack { name, .. }
                | Instruction::StackString { name, .. } => {
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

pub fn validate_program_symbols(program: &Program) -> Result<(), String> {
    let memory = &program.memory;
    let labels = &program.labels;
    let memory_names: HashSet<&str> = memory
        .iter()
        .map(|declaration| match declaration {
            MemoryDeclaration::Scalar { name, .. }
            | MemoryDeclaration::FloatScalar { name, .. }
            | MemoryDeclaration::Buffer { name, .. } => name.as_str(),
        })
        .collect();
    let mut label_names = HashSet::new();
    let top_level_label_names: HashSet<&str> =
        labels.iter().map(|label| label.name.as_str()).collect();

    for label in labels {
        if memory_names.contains(label.name.as_str()) {
            return Err(format!(
                "Label {:?} conflicts with top-level memory",
                label.name
            ));
        }

        if !label_names.insert(label.name.as_str()) {
            return Err(format!("Label {:?} is already defined", label.name));
        }
        for instruction in &label.instructions {
            if let Instruction::Label { name } = instruction {
                if memory_names.contains(name.as_str()) {
                    return Err(format!("Label {:?} conflicts with top-level memory", name));
                }

                if !label_names.insert(name.as_str()) {
                    return Err(format!("Label {:?} is already defined", name));
                }
            }
        }
    }

    for label in labels {
        let mut bindings = HashSet::new();
        let mut operand_bindings = HashSet::new();
        let mut string_bindings = HashSet::new();
        for instruction in &label.instructions {
            match instruction {
                Instruction::Const { name, value } => {
                    if label_names.contains(name.as_str()) {
                        return Err(format!(
                            "Name {name:?} in label {:?} conflicts with top-level label",
                            label.name
                        ));
                    }

                    bindings.insert(name.as_str());
                    if matches!(
                        value,
                        BindingValue::Integer { .. } | BindingValue::Float { .. }
                    ) {
                        operand_bindings.insert(name.as_str());
                    }
                }
                Instruction::Stack { name, .. } => {
                    if label_names.contains(name.as_str()) {
                        return Err(format!(
                            "Name {name:?} in label {:?} conflicts with top-level label",
                            label.name
                        ));
                    }

                    bindings.insert(name.as_str());
                    operand_bindings.insert(name.as_str());
                }
                Instruction::StackString { name, .. } => {
                    if label_names.contains(name.as_str()) {
                        return Err(format!(
                            "Name {name:?} in label {:?} conflicts with top-level label",
                            label.name
                        ));
                    }

                    bindings.insert(name.as_str());
                    string_bindings.insert(name.as_str());
                }
                _ => {}
            }
        }

        for instruction in &label.instructions {
            validate_instruction_symbols(
                instruction,
                &bindings,
                &operand_bindings,
                &string_bindings,
                &memory_names,
                &label_names,
                &top_level_label_names,
                &label.name,
            )?;
        }
    }

    Ok(())
}

fn validate_instruction_symbols(
    instruction: &Instruction,
    bindings: &HashSet<&str>,
    operand_bindings: &HashSet<&str>,
    string_bindings: &HashSet<&str>,
    memory: &HashSet<&str>,
    labels: &HashSet<&str>,
    top_level_labels: &HashSet<&str>,
    current_label: &str,
) -> Result<(), String> {
    match instruction {
        Instruction::Call { target } => {
            if !top_level_labels.contains(target.as_str()) {
                return Err(format!(
                    "call target {target:?} in label {current_label:?} must be a top-level function"
                ));
            }
        }
        Instruction::Jmp { target, .. } => {
            if !labels.contains(target.as_str()) {
                return Err(format!(
                    "Unknown label {target:?} in label {current_label:?}"
                ));
            }

            // keep top_level_labels check for defensive AST invariants
            if !is_local_label_name(target) || top_level_labels.contains(target.as_str()) {
                return Err(format!(
                    "jmp target {target:?} in label {current_label:?} must be a local label"
                ));
            }
        }
        Instruction::Print { parts } => {
            for part in parts {
                if let PrintPart::Binding(name) = part
                    && !bindings.contains(name.as_str())
                {
                    return Err(format!(
                        "Unknown binding {name:?} in label {current_label:?}"
                    ));
                }
            }
        }
        Instruction::Read { dst, .. } => {
            if let Operand::Pointer(name) = dst
                && !memory.contains(name.as_str())
            {
                return Err(format!(
                    "Read destination {name:?} in label {current_label:?} must be top-level memory"
                ));
            }
        }
        _ => {}
    }

    for operand in instruction.operands() {
        validate_operand_symbol(
            operand,
            bindings,
            operand_bindings,
            string_bindings,
            memory,
            labels,
            current_label,
        )?;
    }
    Ok(())
}

fn is_local_label_name(name: &str) -> bool {
    name.starts_with(".L.")
}

fn validate_operand_symbol(
    operand: &Operand,
    bindings: &HashSet<&str>,
    operand_bindings: &HashSet<&str>,
    string_bindings: &HashSet<&str>,
    memory: &HashSet<&str>,
    labels: &HashSet<&str>,
    current_label: &str,
) -> Result<(), String> {
    match operand {
        Operand::Ident(name) if !operand_bindings.contains(name.as_str()) => {
            if bindings.contains(name.as_str()) {
                Err(format!(
                    "String binding {name:?} in label {current_label:?} cannot be used as an operand"
                ))
            } else {
                Err(format!(
                    "Unknown symbol {name:?} in label {current_label:?}"
                ))
            }
        }
        Operand::StringProperty { name, .. } if !bindings.contains(name.as_str()) => Err(format!(
            "Unknown string binding {name:?} in label {current_label:?}"
        )),
        Operand::StringProperty { name, .. } if !string_bindings.contains(name.as_str()) => Err(
            format!("Stack variable {name:?} in label {current_label:?} is not a string"),
        ),
        Operand::Pointer(name)
            if !memory.contains(name.as_str()) && !labels.contains(name.as_str()) =>
        {
            Err(format!(
                "Unknown address target {name:?} in label {current_label:?}"
            ))
        }
        Operand::Dereference { address, .. } => {
            for term in
                std::iter::once(&address.first).chain(address.rest.iter().map(|(_, term)| term))
            {
                if let AddressTerm::Ident(name) = term
                    && !memory.contains(name.as_str())
                    && !labels.contains(name.as_str())
                {
                    return Err(format!(
                        "Unknown address symbol {name:?} in label {current_label:?}"
                    ));
                }
            }
            Ok(())
        }
        _ => Ok(()),
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
    crate::register::is_register(s)
}

fn is_xmm_register_name(s: &str) -> bool {
    crate::register::is_xmm(s)
}
