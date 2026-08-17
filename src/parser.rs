use crate::ast::{
    Address, AddressOperator, AddressTerm, AssignmentTarget, AssignmentValue, BindingValue,
    CompareOp, Condition, ConditionExpr, ControlTarget, DataDeclaration, DataItem, ExprOp,
    Expression, FloatMathOp, ImportDeclaration, Instruction, IntrinsicOp, Label, MathOp,
    MemoryDeclaration, MemoryValue, MemoryWidth, Operand, PairBinaryOp, PrintFormat, PrintPart,
    Program, ReadSource, RegisterPair, StringInitializer, StringProperty, WidthConversion,
};
use crate::diagnostic::{Diagnostic, ProgramOrigins, Span};
use crate::grammar::Token;
use crate::lexer::SpannedToken;
use std::collections::HashSet;

pub struct Parser {
    tokens: Vec<Token>,
    spans: Option<Vec<Span>>,
    position: usize,
    origins: ProgramOrigins,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            spans: None,
            position: 0,
            origins: ProgramOrigins::default(),
        }
    }

    pub fn new_spanned(tokens: Vec<SpannedToken>) -> Self {
        let (tokens, spans): (Vec<_>, Vec<_>) = tokens
            .into_iter()
            .map(|token| (token.token, token.span))
            .unzip();
        Self {
            tokens,
            spans: Some(spans),
            position: 0,
            origins: ProgramOrigins::default(),
        }
    }

    pub fn parse_program(&mut self) -> Result<Program, String> {
        self.parse_program_with_options(true)
    }

    pub fn parse_library(&mut self) -> Result<Program, String> {
        self.parse_program_with_options(false)
    }

    pub fn parse_program_with_diagnostics(&mut self) -> Result<Program, Diagnostic> {
        self.parse_program()
            .map_err(|message| self.diagnostic(message))
    }

    pub fn parse_library_with_diagnostics(&mut self) -> Result<Program, Diagnostic> {
        self.parse_library()
            .map_err(|message| self.diagnostic(message))
    }

    pub fn take_origins(&mut self) -> ProgramOrigins {
        std::mem::take(&mut self.origins)
    }

    fn diagnostic(&self, message: String) -> Diagnostic {
        let Some(spans) = &self.spans else {
            return Diagnostic::new(message);
        };
        if spans.is_empty() {
            return Diagnostic::new(message);
        }
        let index = self
            .position
            .saturating_sub(1)
            .min(spans.len().saturating_sub(1));
        Diagnostic::new(message).at(spans[index])
    }

    fn parse_program_with_options(&mut self, require_main: bool) -> Result<Program, String> {
        let mut imports = Vec::new();
        let mut exports = Vec::new();
        let mut data = Vec::new();
        let mut memory = Vec::new();
        let mut labels = Vec::new();

        while !self.is_at_end() {
            if matches!(self.peek(), Some(Token::Import)) {
                imports.push(self.parse_import_declaration()?);
            } else if matches!(self.peek(), Some(Token::Data)) {
                data.push(self.parse_data_declaration()?);
            } else if matches!(self.peek(), Some(Token::Mem)) {
                memory.push(self.parse_memory_declaration()?);
            } else if matches!(self.peek(), Some(Token::Export)) {
                let label = self.parse_exported_top_level_label()?;
                exports.push(label.name.clone());
                labels.push(label);
            } else {
                labels.push(self.parse_top_level_label()?);
            }
        }

        validate_data_names(&data)?;
        validate_memory_names(&memory)?;
        validate_label_storage_names(&data, &memory, &labels)?;
        if require_main {
            validate_main_label(&labels)?;
        }

        Ok(Program {
            entry: String::from("main"),
            imports,
            exports,
            data,
            memory,
            labels,
        })
    }

    fn parse_import_declaration(&mut self) -> Result<ImportDeclaration, String> {
        self.expect(Token::Import, "Expected import declaration")?;

        let mut names = Vec::new();
        loop {
            names.push(self.expect_ident("imported function name")?);

            match self.peek() {
                Some(Token::Comma) => {
                    self.advance();
                }
                Some(Token::From) => break,
                Some(token) => {
                    return Err(format!(
                        "Expected ',' or from after imported function name, found {token:?}"
                    ));
                }
                None => {
                    return Err(String::from(
                        "Expected ',' or from after imported function name, found end of input",
                    ));
                }
            }
        }

        self.expect(Token::From, "Expected from after imported function names")?;
        let path = match self.advance() {
            Some(Token::Text(path)) => path,
            Some(token) => return Err(format!("Expected import path string, found {token:?}")),
            None => {
                return Err(String::from(
                    "Expected import path string, found end of input",
                ));
            }
        };

        Ok(ImportDeclaration { names, path })
    }

    fn parse_data_declaration(&mut self) -> Result<DataDeclaration, String> {
        self.expect(Token::Data, "Expected data declaration")?;

        let name = self.expect_ident("data name after data")?;
        let mut section = None;
        let mut align = None;
        let mut export = false;
        let mut keep = false;

        while !matches!(self.peek(), Some(Token::LBrace)) {
            match self.advance() {
                Some(Token::Section) => {
                    if section.is_some() {
                        return Err(format!("Data block {name:?} already has a section"));
                    }

                    section = match self.advance() {
                        Some(Token::Text(value)) => Some(value),
                        Some(token) => {
                            return Err(format!(
                                "Expected section name string after section, found {token:?}"
                            ));
                        }
                        None => {
                            return Err(String::from(
                                "Expected section name string after section, found end of input",
                            ));
                        }
                    };
                }
                Some(Token::Align) => {
                    if align.is_some() {
                        return Err(format!("Data block {name:?} already has an alignment"));
                    }

                    let value = self.parse_usize_literal("data alignment")?;
                    if value == 0 || !value.is_power_of_two() {
                        return Err(format!(
                            "Data block {name:?} alignment must be a non-zero power of two"
                        ));
                    }

                    align = Some(value);
                }
                Some(Token::Export) => {
                    if export {
                        return Err(format!("Data block {name:?} already has export"));
                    }

                    export = true;
                }
                Some(Token::Keep) => {
                    if keep {
                        return Err(format!("Data block {name:?} already has keep"));
                    }

                    keep = true;
                }
                Some(token) => {
                    return Err(format!(
                        "Expected data option section, align, export, keep, or '{{', found {token:?}"
                    ));
                }
                None => return Err(format!("Expected '{{' to start data block {name:?}")),
            }
        }

        let section = section.ok_or_else(|| format!("Data block {name:?} must specify section"))?;
        self.expect(Token::LBrace, "Expected '{' to start data block")?;

        let mut items = Vec::new();
        while !matches!(self.peek(), Some(Token::RBrace)) {
            if self.is_at_end() {
                return Err(format!("Expected '}}' to close data block {name:?}"));
            }

            items.push(self.parse_data_item(&name)?);
        }

        self.expect(Token::RBrace, "Expected '}' after data block")?;

        Ok(DataDeclaration {
            name,
            section,
            align,
            export,
            keep,
            items,
        })
    }

    fn parse_data_item(&mut self, data_name: &str) -> Result<DataItem, String> {
        match self.advance() {
            Some(Token::Ident(name)) => {
                if matches!(self.peek(), Some(Token::Colon)) {
                    self.advance();
                    return Ok(DataItem::Label { name });
                }

                let width = MemoryWidth::parse(&name)?;
                if width.is_float() {
                    return Err(format!(
                        "Data block {data_name:?} does not support floating-point scalar {}",
                        width.name()
                    ));
                }

                let value = self.parse_integer_literal_for_width("data scalar", width)?;
                validate_integer_binding_width(value, width)?;
                Ok(DataItem::Scalar { width, value })
            }
            Some(Token::Addr) => {
                let target = self.expect_ident("address target after addr")?;
                Ok(DataItem::Addr { target })
            }
            Some(Token::Zero) => {
                let count = self.parse_usize_literal("zero byte count")?;
                Ok(DataItem::Zero { count })
            }
            Some(token) => Err(format!(
                "Expected data item width, addr, zero, or label in data block {data_name:?}, found {token:?}"
            )),
            None => Err(format!(
                "Expected data item in data block {data_name:?}, found end of input"
            )),
        }
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

            let start = self.position;
            instructions.push(self.parse_instruction(&name)?);
            self.record_instruction_span(&name, start);
        }

        self.expect(Token::RBrace, "Expected '}' after label block")?;

        Ok(Label { name, instructions })
    }

    fn parse_exported_top_level_label(&mut self) -> Result<Label, String> {
        self.expect(Token::Export, "Expected export")?;

        let name = match self.advance() {
            Some(Token::Ident(name)) => name,
            Some(Token::LocalIdent(name)) => {
                return Err(format!(
                    "Local label .{name} cannot be exported at the top level"
                ));
            }
            Some(token) => return Err(format!("Expected exported function name, found {token:?}")),
            None => {
                return Err(String::from(
                    "Expected exported function name, found end of input",
                ));
            }
        };

        self.expect(Token::Colon, "Expected ':' after exported function name")?;
        if !matches!(self.peek(), Some(Token::LBrace)) {
            return Err(format!("Exported function {name:?} must have a block"));
        }

        self.expect(
            Token::LBrace,
            "Expected '{' to start exported function block",
        )?;

        let mut instructions = Vec::new();
        while !matches!(self.peek(), Some(Token::RBrace)) {
            if self.is_at_end() {
                return Err(format!("Expected '}}' to close exported function '{name}'"));
            }

            let start = self.position;
            instructions.push(self.parse_instruction(&name)?);
            self.record_instruction_span(&name, start);
        }

        self.expect(Token::RBrace, "Expected '}' after exported function block")?;

        Ok(Label { name, instructions })
    }

    fn record_instruction_span(&mut self, label: &str, start: usize) {
        let Some(spans) = &self.spans else {
            return;
        };
        let Some(first) = spans.get(start) else {
            return;
        };
        let end = self
            .position
            .saturating_sub(1)
            .min(spans.len().saturating_sub(1));
        self.origins
            .record_instruction(label, Span::new(first.source, first.start, spans[end].end));
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

                if matches!(self.peek(), Some(Token::Text(_))) {
                    if width != MemoryWidth::U8 {
                        return Err(String::from(
                            "String memory initializers require u8 memory width",
                        ));
                    }

                    let Some(Token::Text(value)) = self.advance() else {
                        unreachable!()
                    };
                    return Ok(MemoryDeclaration::Array {
                        name,
                        width,
                        values: value
                            .bytes()
                            .map(|value| MemoryValue::Integer(value as i128))
                            .collect(),
                    });
                }

                if matches!(self.peek(), Some(Token::LBracket)) {
                    let values = self.parse_memory_array_values(width)?;
                    return Ok(MemoryDeclaration::Array {
                        name,
                        width,
                        values,
                    });
                }

                if matches!(self.peek(), Some(Token::Repeat)) {
                    self.advance();
                    self.expect(Token::LParen, "Expected '(' after repeat")?;
                    let count = self.parse_usize_literal("repeat count")?;
                    self.expect(Token::Comma, "Expected ',' after repeat count")?;
                    let value = self.parse_memory_value(width)?;
                    self.expect(Token::RParen, "Expected ')' after repeat value")?;
                    return Ok(MemoryDeclaration::Repeat {
                        name,
                        width,
                        count,
                        value,
                    });
                }

                if matches!(self.peek(), Some(Token::Addr)) {
                    let value = self.parse_memory_value(width)?;
                    return Ok(MemoryDeclaration::Array {
                        name,
                        width,
                        values: vec![value],
                    });
                }

                if width == MemoryWidth::Ptr {
                    return Err(String::from(
                        "ptr memory initializers require addr <symbol>",
                    ));
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

    fn parse_memory_array_values(
        &mut self,
        width: MemoryWidth,
    ) -> Result<Vec<MemoryValue>, String> {
        self.expect(
            Token::LBracket,
            "Expected '[' to start memory array initializer",
        )?;
        let mut values = Vec::new();

        if matches!(self.peek(), Some(Token::RBracket)) {
            return Err(String::from("Memory array initializer cannot be empty"));
        }

        loop {
            values.push(self.parse_memory_value(width)?);

            match self.peek() {
                Some(Token::Comma) => {
                    self.advance();
                    if matches!(self.peek(), Some(Token::RBracket)) {
                        return Err(String::from("Memory array initializer cannot end with ','"));
                    }
                }
                Some(Token::RBracket) => {
                    self.advance();
                    break;
                }
                Some(token) => {
                    return Err(format!(
                        "Expected ',' or ']' after memory array value, found {token:?}"
                    ));
                }
                None => {
                    return Err(String::from(
                        "Expected ',' or ']' after memory array value, found end of input",
                    ));
                }
            }
        }

        Ok(values)
    }

    fn parse_memory_value(&mut self, width: MemoryWidth) -> Result<MemoryValue, String> {
        match self.peek() {
            Some(Token::Addr) => {
                if width != MemoryWidth::Ptr {
                    return Err(String::from(
                        "Address memory initializers require ptr memory width",
                    ));
                }

                self.advance();
                let target = self.expect_ident("address target after addr")?;
                Ok(MemoryValue::Addr { target })
            }
            _ => {
                if width == MemoryWidth::Ptr {
                    return Err(String::from(
                        "ptr memory initializers require addr <symbol>",
                    ));
                }

                let value = self.parse_integer_literal("memory initializer")?;
                validate_integer_binding_width(value, width)?;
                Ok(MemoryValue::Integer(value))
            }
        }
    }

    fn parse_namespaced_instruction(&mut self, namespace: String) -> Result<Instruction, String> {
        let operation = match self.advance() {
            Some(Token::LocalIdent(operation)) => operation,
            Some(token) => {
                return Err(format!(
                    "Expected namespaced instruction after {namespace}, found {token:?}"
                ));
            }
            None => {
                return Err(format!(
                    "Expected namespaced instruction after {namespace}, found end of input"
                ));
            }
        };

        match (namespace.as_str(), operation.as_str()) {
            ("linux", "exit") => {
                let code = self.parse_exit_code()?;
                Ok(Instruction::Exit { code })
            }
            ("linux", "print") => self.parse_print_instruction(),
            ("linux", "read") => self.parse_read_instruction(),
            ("linux", "release") => self.parse_release_instruction(),
            ("linux", "reserve") => Err(String::from(
                "linux.reserve(size) returns a pointer; assign it to a destination",
            )),
            ("linux", "syscall") => Ok(Instruction::Syscall),
            ("x86", operation) => Err(format!(
                "Unknown instruction \"x86.{operation}\"; use x86 \"{operation}\" for raw x86 assembly"
            )),
            _ => Err(format!("Unknown instruction \"{namespace}.{operation}\"")),
        }
    }

    fn parse_inline_x86(&mut self) -> Result<Instruction, String> {
        match self.advance() {
            Some(Token::Text(text)) => {
                if text.contains(['\n', '\r']) {
                    Err(String::from("x86 assembly must be a single line"))
                } else if text.trim().is_empty() {
                    Err(String::from("x86 assembly cannot be empty"))
                } else {
                    Ok(Instruction::InlineAsm { text })
                }
            }
            Some(token) => Err(format!(
                "Expected string literal after x86, found {token:?}"
            )),
            None => Err(String::from(
                "Expected string literal after x86, found end of input",
            )),
        }
    }

    fn parse_print_instruction(&mut self) -> Result<Instruction, String> {
        match self.advance() {
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
                parts: vec![PrintPart::FormattedOperand {
                    format: PrintFormat::SignedDecimal(MemoryWidth::I64),
                    operand: Operand::Register(name),
                }],
            }),
            Some(Token::NumberLiteral(value)) => value
                .parse::<i128>()
                .map(|value| Instruction::Print {
                    parts: vec![PrintPart::FormattedOperand {
                        format: PrintFormat::SignedDecimal(MemoryWidth::I64),
                        operand: Operand::Immediate(value),
                    }],
                })
                .map_err(|_| format!("Invalid integer literal {value:?}")),
            Some(Token::FloatLiteral(value)) => Err(format!(
                "Float literal {value} cannot be printed directly yet; bind it with const name:f32 or const name:f64 first"
            )),
            Some(Token::Minus) => match self.advance() {
                Some(Token::NumberLiteral(value)) => parse_signed_integer(&value, true)
                    .map_err(|_| format!("Invalid integer literal -{value}"))
                    .map(|value| Instruction::Print {
                        parts: vec![PrintPart::FormattedOperand {
                            format: PrintFormat::SignedDecimal(MemoryWidth::I64),
                            operand: Operand::Immediate(value),
                        }],
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
                "Expected binding name, register, integer, or string literal after linux.print, found {token:?}"
            )),
            None => Err(String::from(
                "Expected binding name, register, integer, or string literal after linux.print, found end of input",
            )),
        }
    }

    fn parse_instruction(&mut self, current_label: &str) -> Result<Instruction, String> {
        match self.advance() {
            Some(Token::Call) => {
                let target = self.parse_control_target("call", current_label)?;
                Ok(Instruction::Call { target })
            }
            Some(Token::Jmp) => {
                let target = self.parse_control_target("jump", current_label)?;
                let condition = self.parse_optional_jump_condition()?;
                Ok(Instruction::Jmp { target, condition })
            }
            Some(Token::Exit) => Err(suggest_namespaced_instruction("exit", "linux.exit")),
            Some(Token::Halt) => Err(suggest_raw_x86_instruction("hlt")),
            Some(Token::In) => Err(suggest_raw_x86_instruction("in")),
            Some(Token::Const) => self.parse_const_declaration(),
            Some(Token::Print) => Err(suggest_namespaced_instruction("print", "linux.print")),
            Some(Token::Nop) => Ok(Instruction::Nop),
            Some(Token::Pop) => {
                let dst = self.parse_operand()?;
                Ok(Instruction::Pop { dst })
            }
            Some(Token::Out) => Err(suggest_raw_x86_instruction("out")),
            Some(Token::Push) => {
                let src = self.parse_operand()?;
                Ok(Instruction::Push { src })
            }
            Some(Token::Read) => Err(suggest_namespaced_instruction("read", "linux.read")),
            Some(Token::Ret) => Ok(Instruction::Ret),
            Some(Token::Stack) => self.parse_stack_declaration(),
            Some(Token::Syscall) => Err(suggest_namespaced_instruction("syscall", "linux.syscall")),
            Some(Token::X86) => self.parse_inline_x86(),
            Some(Token::Ampersand) => Err(String::from(
                "Address-of syntax is only supported on the right side of assignment",
            )),
            Some(Token::LocalIdent(name)) if matches!(self.peek(), Some(Token::Colon)) => {
                self.advance();
                Ok(Instruction::Label {
                    name: mangle_local_label(current_label, &name),
                })
            }
            Some(Token::Ident(name)) if matches!(self.peek(), Some(Token::LocalIdent(_))) => {
                self.parse_namespaced_instruction(name)
            }
            Some(Token::Ident(name)) if matches!(self.peek(), Some(Token::Colon)) => Err(format!(
                "Nested label {name}: must be local; write .{name}: instead"
            )),
            Some(Token::Ident(name)) if matches!(self.peek(), Some(Token::LBracket)) => {
                let address = self.parse_indexed_address(name)?;
                let width = self.parse_optional_memory_width()?;

                self.parse_assignment(AssignmentTarget::Operand(Operand::Dereference {
                    address,
                    width,
                }))
            }
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
        self.expect(Token::LParen, "Expected '(' after linux.read")?;
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
        self.expect(Token::RParen, "Expected ')' after read buffer size")?;

        Ok(Instruction::Read { src, dst, len })
    }

    fn parse_release_instruction(&mut self) -> Result<Instruction, String> {
        self.expect(Token::LParen, "Expected '(' after linux.release")?;
        let ptr = self.parse_operand()?;
        self.expect(Token::Comma, "Expected ',' after release pointer")?;
        let len = self.parse_operand()?;
        self.expect(Token::RParen, "Expected ')' after release size")?;

        Ok(Instruction::Release { ptr, len })
    }

    fn parse_string_initializer(&mut self) -> Result<StringInitializer, String> {
        match self.advance() {
            Some(Token::Text(value)) => Ok(StringInitializer::Literal(value)),
            Some(Token::Slice) => {
                self.expect(Token::LParen, "Expected '(' after slice")?;
                let ptr = self.parse_operand()?;
                self.expect(Token::Comma, "Expected ',' after slice pointer")?;
                let len = self.parse_operand()?;
                self.expect(Token::RParen, "Expected ')' after slice length")?;

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

    fn parse_control_target(
        &mut self,
        instruction: &str,
        current_label: &str,
    ) -> Result<ControlTarget, String> {
        match self.advance() {
            Some(Token::Ident(target)) if matches!(self.peek(), Some(Token::LBracket)) => {
                let address = self.parse_indexed_address(target)?;
                let width = self.parse_optional_memory_width()?;
                Ok(ControlTarget::Operand(Operand::Dereference {
                    address,
                    width,
                }))
            }
            Some(Token::Ident(target)) => Ok(ControlTarget::Label(target)),
            Some(Token::LocalIdent(target)) => Ok(ControlTarget::Label(mangle_local_label(
                current_label,
                &target,
            ))),
            Some(Token::LBracket) => {
                let address = self.parse_address()?;
                self.expect(Token::RBracket, "Expected ']' after memory operand")?;
                let width = self.parse_optional_memory_width()?;
                Ok(ControlTarget::Operand(Operand::Dereference {
                    address,
                    width,
                }))
            }
            Some(Token::Register(target)) => Ok(ControlTarget::Operand(Operand::Register(target))),
            Some(token) => Err(format!(
                "Expected {instruction} target label, register, or memory operand, found {token:?}"
            )),
            None => Err(format!(
                "Expected {instruction} target label, register, or memory operand, found end of input"
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

        self.parse_assignment(AssignmentTarget::RegisterPair(RegisterPair {
            high: high_or_dst,
            low,
        }))
    }

    fn parse_assignment(&mut self, dst: AssignmentTarget) -> Result<Instruction, String> {
        self.expect(Token::Equals, "Expected '=' after assignment destination")?;

        if matches!(dst, AssignmentTarget::RegisterPair(_)) {
            return self.parse_wide_assignment(dst);
        }

        let value = if matches!(self.peek(), Some(Token::Tilde)) {
            self.advance();
            let operand = self.parse_operand()?;
            AssignmentValue::BitwiseUnary {
                op: crate::ast::BitwiseUnaryOp::Not,
                operand,
            }
        } else if self.next_tokens_are_intrinsic_call() {
            self.parse_intrinsic_call_assignment_value()?
        } else if self.next_tokens_are_linux_reserve_call() {
            self.parse_linux_reserve_assignment_value()?
        } else if matches!(self.peek(), Some(Token::Text(_))) {
            self.parse_string_bytes_assignment_value()?
        } else {
            let expression = self.parse_expression(0)?;
            match self.peek() {
                Some(Token::Slash) => {
                    return Err(String::from(
                        "Division must specify signedness; use i/ or u/",
                    ));
                }
                Some(Token::Percent) => {
                    return Err(String::from("Modulo must specify signedness; use i% or u%"));
                }
                _ => {}
            }

            if let Expression::Operand(lhs) = &expression
                && let Some(AssignmentOp::FloatBinary(_, _) | AssignmentOp::UnsupportedDivision) =
                    self.peek().and_then(assignment_op)
            {
                let op = self.peek().and_then(assignment_op).unwrap();
                if matches!(op, AssignmentOp::UnsupportedDivision) {
                    self.advance();
                    return Err(String::from(
                        "Use rdx:rax = lhs u/ rhs or rdx:rax = lhs i/ rhs for division",
                    ));
                }
                self.advance();
                let rhs = self.parse_operand()?;
                assignment_value(op, lhs.clone(), rhs)
            } else if self.peek().and_then(compare_op).is_some() {
                self.assignment_condition_value(expression)?
            } else {
                expression_assignment_value(expression)
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

    fn next_tokens_are_intrinsic_call(&self) -> bool {
        matches!(self.peek(), Some(Token::Ident(_)))
            && matches!(self.tokens.get(self.position + 1), Some(Token::LParen))
    }

    fn next_tokens_are_linux_reserve_call(&self) -> bool {
        matches!(self.peek(), Some(Token::Ident(namespace)) if namespace == "linux")
            && matches!(self.tokens.get(self.position + 1), Some(Token::LocalIdent(operation)) if operation == "reserve")
    }

    fn parse_linux_reserve_assignment_value(&mut self) -> Result<AssignmentValue, String> {
        self.advance();
        self.advance();
        self.expect(Token::LParen, "Expected '(' after linux.reserve")?;
        let len = self.parse_operand()?;
        self.expect(Token::RParen, "Expected ')' after reserve size")?;

        Ok(AssignmentValue::LinuxReserve { len })
    }

    fn parse_string_bytes_assignment_value(&mut self) -> Result<AssignmentValue, String> {
        let Some(Token::Text(value)) = self.advance() else {
            unreachable!()
        };

        if value.is_empty() {
            return Err(String::from("String byte assignment cannot be empty"));
        }

        Ok(AssignmentValue::StringBytes { value })
    }

    fn parse_intrinsic_call_assignment_value(&mut self) -> Result<AssignmentValue, String> {
        let name = match self.advance() {
            Some(Token::Ident(name)) => name,
            _ => unreachable!(),
        };
        let op = parse_intrinsic_op(&name)?;

        self.expect(Token::LParen, "Expected '(' after intrinsic name")?;
        let args = self.parse_intrinsic_args(&name)?;
        self.expect(Token::Colon, "Expected ':' after typed intrinsic call")?;
        let width_name = self.expect_ident("typed intrinsic width after ':'")?;
        let width = MemoryWidth::parse(&width_name)?;

        validate_intrinsic_arity(op, args.len())?;

        Ok(AssignmentValue::IntrinsicCall { op, width, args })
    }

    fn parse_intrinsic_args(&mut self, name: &str) -> Result<Vec<Operand>, String> {
        let mut args = Vec::new();

        if matches!(self.peek(), Some(Token::RParen)) {
            self.advance();
            return Ok(args);
        }

        loop {
            args.push(self.parse_operand()?);

            match self.peek() {
                Some(Token::Comma) => {
                    self.advance();
                    if matches!(self.peek(), Some(Token::RParen)) {
                        return Err(format!("Typed intrinsic call {name} cannot end with ','"));
                    }
                }
                Some(Token::RParen) => {
                    self.advance();
                    return Ok(args);
                }
                Some(token) => {
                    return Err(format!(
                        "Expected ',' or ')' after typed intrinsic argument, found {token:?}"
                    ));
                }
                None => {
                    return Err(String::from(
                        "Expected ',' or ')' after typed intrinsic argument, found end of input",
                    ));
                }
            }
        }
    }

    fn parse_wide_assignment(&mut self, dst: AssignmentTarget) -> Result<Instruction, String> {
        if matches!(self.peek(), Some(Token::Register(_)))
            && matches!(self.tokens.get(self.position + 1), Some(Token::Colon))
        {
            let lhs = self.parse_register_pair_operand()?;
            let op = match self.advance() {
                Some(Token::Plus) => PairBinaryOp::Add,
                Some(Token::Minus) => PairBinaryOp::Subtract,
                Some(token) => {
                    return Err(format!(
                        "Register-pair assignment expected '+' or '-' after register-pair left operand, found {token:?}"
                    ));
                }
                None => {
                    return Err(String::from(
                        "Register-pair assignment expected '+' or '-' after register-pair left operand, found end of input",
                    ));
                }
            };
            let rhs = self.parse_register_pair_operand()?;
            let value = AssignmentValue::PairBinary { op, lhs, rhs };

            if let Some(condition) = self.parse_optional_assignment_condition()? {
                return Ok(Instruction::AssignIf {
                    dst,
                    value,
                    condition,
                });
            }

            return Ok(Instruction::Assign { dst, value });
        }

        let lhs = self.parse_operand()?;
        let Some(op) = self.peek().and_then(assignment_op) else {
            return Err(String::from(
                "Register-pair assignment needs a widened operator after the left operand; e.g. `rdx:rax = lhs u* rhs`",
            ));
        };

        if !matches!(op, AssignmentOp::Wide { .. }) {
            return Err(String::from(
                "Register-pair assignment only supports widened `u*`, `i*`, `u/`, or `i/`.",
            ));
        }

        self.advance();
        let rhs = self.parse_operand()?;
        let value = assignment_value(op, lhs, rhs);

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

    fn parse_register_pair_operand(&mut self) -> Result<RegisterPair, String> {
        let high = self.expect_register("high register in register-pair operand")?;
        self.expect(Token::Colon, "Expected ':' in register-pair operand")?;
        let low = self.expect_register("low register in register-pair operand")?;

        Ok(RegisterPair { high, low })
    }

    fn parse_expression(&mut self, min_precedence: u8) -> Result<Expression, String> {
        let mut lhs = self.parse_expression_primary()?;

        while let Some((op, precedence, right_associative)) = self.peek().and_then(expression_op) {
            if precedence < min_precedence {
                break;
            }

            self.advance();
            let next_min_precedence = if right_associative {
                precedence
            } else {
                precedence + 1
            };
            let rhs = self.parse_expression(next_min_precedence)?;
            lhs = Expression::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }

        Ok(lhs)
    }

    fn parse_expression_primary(&mut self) -> Result<Expression, String> {
        if matches!(self.peek(), Some(Token::LParen)) {
            self.advance();
            let expression = self.parse_expression(0)?;
            self.expect(Token::RParen, "Expected ')' after expression")?;
            Ok(expression)
        } else {
            self.parse_operand().map(Expression::Operand)
        }
    }

    fn assignment_condition_value(
        &mut self,
        expression: Expression,
    ) -> Result<AssignmentValue, String> {
        match expression {
            Expression::Operand(lhs) => Ok(AssignmentValue::Condition(
                self.parse_condition_expr_after_lhs(lhs)?,
            )),
            Expression::Binary {
                op: ExprOp::Math(MathOp::BitAnd),
                lhs,
                rhs,
            } => {
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

                let (Expression::Operand(lhs), Expression::Operand(rhs)) = (*lhs, *rhs) else {
                    return Err(String::from(
                        "Bitwise-and conditions only support simple operands",
                    ));
                };

                Ok(AssignmentValue::Condition(ConditionExpr::BitwiseAndZero {
                    lhs,
                    rhs,
                    op,
                }))
            }
            _ => Err(String::from(
                "Comparison assignments require a simple operand on the left side",
            )),
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

            args.push(self.parse_print_arg()?);
        }

        let segments = split_format_literal(&value)?;
        let placeholder_count = segments
            .iter()
            .filter(|segment| matches!(segment, FormatSegment::Placeholder(_)))
            .count();
        if placeholder_count != args.len() {
            return Err(format!(
                "Print format expected {} argument(s), found {}",
                placeholder_count,
                args.len()
            ));
        }

        let mut parts = Vec::new();
        let mut args = args.into_iter();
        for segment in segments {
            match segment {
                FormatSegment::Literal(literal) => {
                    if !literal.is_empty() {
                        parts.push(PrintPart::Literal(literal));
                    }
                }
                FormatSegment::Placeholder(format) => {
                    let arg = args
                        .next()
                        .ok_or_else(|| String::from("Internal error: missing print argument"))?;
                    parts.push(print_part_for_format_arg(format, arg));
                }
            }
        }

        Ok(Instruction::Print { parts })
    }

    fn parse_print_arg(&mut self) -> Result<PrintArg, String> {
        if let Some(Token::Ident(name)) = self.peek().cloned()
            && !matches!(
                self.tokens.get(self.position + 1),
                Some(Token::LBracket | Token::LocalIdent(_))
            )
        {
            self.advance();
            return Ok(PrintArg::Ident(name));
        }

        self.parse_operand().map(PrintArg::Operand)
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
            Some(Token::NumberLiteral(value)) => parse_integer_value(&value, false)
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

    fn parse_integer_literal_for_width(
        &mut self,
        context: &str,
        width: MemoryWidth,
    ) -> Result<i128, String> {
        match self.advance() {
            Some(Token::NumberLiteral(value)) => {
                parse_integer_value_for_width(&value, false, width)
                    .map_err(|_| format!("Invalid integer {context} {value:?}"))
            }
            Some(Token::Minus) => match self.advance() {
                Some(Token::NumberLiteral(value)) => {
                    parse_integer_value_for_width(&value, true, width)
                        .map_err(|_| format!("Invalid integer {context} -{value}"))
                }
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

    fn parse_usize_literal(&mut self, context: &str) -> Result<usize, String> {
        match self.advance() {
            Some(Token::NumberLiteral(value)) => value
                .parse::<usize>()
                .map_err(|_| format!("Invalid {context} {value:?}")),
            Some(Token::Minus) => Err(format!("{context} cannot be negative")),
            Some(token) => Err(format!("Expected {context}, found {token:?}")),
            None => Err(format!("Expected {context}, found end of input")),
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
        let operand = match self.advance() {
            Some(Token::Ampersand) => match self.advance() {
                Some(Token::Ident(name)) if matches!(self.peek(), Some(Token::LBracket)) => {
                    self.parse_indexed_address(name).map(Operand::AddressOf)
                }
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
                Some(Token::LBracket) => {
                    let address = self.parse_address()?;
                    self.expect(Token::RBracket, "Expected ']' after address-of expression")?;
                    Ok(Operand::AddressOf(address))
                }
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
            Some(Token::Ident(name)) if matches!(self.peek(), Some(Token::LBracket)) => {
                let address = self.parse_indexed_address(name)?;
                let width = self.parse_optional_memory_width()?;
                Ok(Operand::Dereference { address, width })
            }
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
        }?;

        self.parse_optional_operand_suffix(operand)
    }

    fn parse_optional_operand_suffix(&mut self, operand: Operand) -> Result<Operand, String> {
        let mut operand = operand;

        loop {
            if matches!(self.peek(), Some(Token::DoubleColon)) {
                operand = self.parse_converted_operand(operand)?;
            } else {
                return Ok(operand);
            }
        }
    }

    fn parse_converted_operand(&mut self, operand: Operand) -> Result<Operand, String> {
        self.advance();
        match self.advance() {
            Some(Token::Ident(name)) if name == "zx" => Ok(Operand::Converted {
                operand: Box::new(operand),
                conversion: WidthConversion::ZeroExtend,
            }),
            Some(Token::Ident(name)) if name == "sx" => Ok(Operand::Converted {
                operand: Box::new(operand),
                conversion: WidthConversion::SignExtend,
            }),
            Some(Token::Ident(name)) => {
                let width = MemoryWidth::parse(&name).map_err(|_| {
                    format!("Unknown conversion ::{name}; expected ::zx, ::sx, or a memory width")
                })?;
                Ok(Operand::Cast {
                    operand: Box::new(operand),
                    width,
                })
            }
            Some(token) => Err(format!(
                "Expected width conversion after '::', found {token:?}"
            )),
            None => Err(String::from(
                "Expected width conversion after '::', found end of input",
            )),
        }
    }

    fn parse_indexed_address(&mut self, base: String) -> Result<Address, String> {
        self.expect(Token::LBracket, "Expected '[' after indexed memory base")?;

        let mut rest = Vec::new();
        if !matches!(self.peek(), Some(Token::RBracket)) {
            rest.push((AddressOperator::Add, self.parse_address_term()?));

            while matches!(self.peek(), Some(Token::Plus | Token::Minus)) {
                let operator = match self.advance() {
                    Some(Token::Plus) => AddressOperator::Add,
                    Some(Token::Minus) => AddressOperator::Subtract,
                    _ => unreachable!(),
                };

                rest.push((operator, self.parse_address_term()?));
            }
        }

        self.expect(Token::RBracket, "Expected ']' after indexed memory offset")?;

        Ok(Address {
            first: AddressTerm::Ident(base),
            rest,
        })
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
        Token::DoubleStar => AssignmentOp::Binary(MathOp::Power),
        Token::Percent => AssignmentOp::UnsupportedDivision,
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
        Token::IPercent => AssignmentOp::UnsupportedDivision,
        Token::IStar => AssignmentOp::Wide {
            signed: true,
            division: false,
        },
        Token::USlash => AssignmentOp::Wide {
            signed: false,
            division: true,
        },
        Token::UPercent => AssignmentOp::UnsupportedDivision,
        Token::UStar => AssignmentOp::Wide {
            signed: false,
            division: false,
        },
        _ => return None,
    })
}

fn expression_op(token: &Token) -> Option<(ExprOp, u8, bool)> {
    let op = match token {
        Token::Pipe => (ExprOp::Math(MathOp::BitOr), 1, false),
        Token::Caret => (ExprOp::Math(MathOp::BitXor), 2, false),
        Token::Ampersand => (ExprOp::Math(MathOp::BitAnd), 3, false),
        Token::ShiftLeft => (ExprOp::Math(MathOp::ShiftLeft), 4, false),
        Token::ShiftRight => (ExprOp::Math(MathOp::ShiftRightLogical), 4, false),
        Token::IShiftRight => (ExprOp::Math(MathOp::ShiftRightArithmetic), 4, false),
        Token::Plus => (ExprOp::Math(MathOp::Add), 5, false),
        Token::Minus => (ExprOp::Math(MathOp::Subtract), 5, false),
        Token::Star => (ExprOp::Math(MathOp::Multiply), 6, false),
        Token::ISlash => (ExprOp::Divide { signed: true }, 6, false),
        Token::USlash => (ExprOp::Divide { signed: false }, 6, false),
        Token::IPercent => (ExprOp::Modulo { signed: true }, 6, false),
        Token::UPercent => (ExprOp::Modulo { signed: false }, 6, false),
        Token::Slash | Token::Percent => return None,
        Token::DoubleStar => (ExprOp::Power, 7, true),
        _ => return None,
    };

    Some(op)
}

fn expression_assignment_value(expression: Expression) -> AssignmentValue {
    match expression {
        Expression::Operand(operand) => AssignmentValue::Operand(operand),
        Expression::Binary { op, lhs, rhs } => match (*lhs, *rhs, op) {
            (Expression::Operand(lhs), Expression::Operand(rhs), ExprOp::Math(op)) => {
                AssignmentValue::Binary { op, lhs, rhs }
            }
            (lhs, rhs, op) => AssignmentValue::Expression(Expression::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            }),
        },
    }
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

fn parse_intrinsic_op(name: &str) -> Result<IntrinsicOp, String> {
    match name {
        "ceil" => Ok(IntrinsicOp::Ceil),
        "floor" => Ok(IntrinsicOp::Floor),
        "max" => Ok(IntrinsicOp::Max),
        "min" => Ok(IntrinsicOp::Min),
        "round" => Ok(IntrinsicOp::Round),
        "sqrt" => Ok(IntrinsicOp::Sqrt),
        "trunc" => Ok(IntrinsicOp::Trunc),
        _ => Err(format!("Unknown typed intrinsic call {name:?}")),
    }
}

fn validate_intrinsic_arity(op: IntrinsicOp, count: usize) -> Result<(), String> {
    let expected = match op {
        IntrinsicOp::Max | IntrinsicOp::Min => 2,
        IntrinsicOp::Ceil
        | IntrinsicOp::Floor
        | IntrinsicOp::Round
        | IntrinsicOp::Sqrt
        | IntrinsicOp::Trunc => 1,
    };

    if count == expected {
        Ok(())
    } else {
        Err(format!(
            "Typed intrinsic call {} expects {expected} argument(s), found {count}",
            intrinsic_op_name(op)
        ))
    }
}

fn intrinsic_op_name(op: IntrinsicOp) -> &'static str {
    match op {
        IntrinsicOp::Ceil => "ceil",
        IntrinsicOp::Floor => "floor",
        IntrinsicOp::Max => "max",
        IntrinsicOp::Min => "min",
        IntrinsicOp::Round => "round",
        IntrinsicOp::Sqrt => "sqrt",
        IntrinsicOp::Trunc => "trunc",
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

    let value = parse_integer_value(value, false)
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
    parse_integer_value(value, negative)
}

fn parse_integer_value(value: &str, negative: bool) -> Result<i128, ()> {
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        let parsed = u128::from_str_radix(hex, 16).map_err(|_| ())?;
        i128::try_from(parsed).map_err(|_| ())?
    } else {
        value.parse::<i128>().map_err(|_| ())?
    };

    if negative { Ok(-parsed) } else { Ok(parsed) }
}

fn parse_integer_value_for_width(
    value: &str,
    negative: bool,
    width: MemoryWidth,
) -> Result<i128, ()> {
    if negative || !matches!(width, MemoryWidth::U64) {
        return parse_integer_value(value, negative);
    }

    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        let parsed = u64::from_str_radix(hex, 16).map_err(|_| ())?;
        Ok(parsed as i128)
    } else {
        parse_integer_value(value, false)
    }
}

fn suggest_namespaced_instruction(instruction: &str, suggestion: &str) -> String {
    format!("Unknown instruction \"{instruction}\"; did you mean {suggestion}?")
}

fn suggest_raw_x86_instruction(instruction: &str) -> String {
    format!("Unknown instruction \"{instruction}\"; use x86 \"{instruction}\" for raw x86 assembly")
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
        MemoryWidth::U64 | MemoryWidth::Ptr => 0 <= value && value <= u64::MAX as i128,
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

fn validate_data_names(data: &[DataDeclaration]) -> Result<(), String> {
    let mut names = HashSet::new();

    for declaration in data {
        if !names.insert(declaration.name.as_str()) {
            return Err(format!(
                "Data name {:?} is already defined",
                declaration.name
            ));
        }

        for item in &declaration.items {
            if let DataItem::Label { name } = item
                && !names.insert(name.as_str())
            {
                return Err(format!("Data label {name:?} is already defined"));
            }
        }
    }

    Ok(())
}

fn validate_memory_names(memory: &[MemoryDeclaration]) -> Result<(), String> {
    let mut names = HashSet::new();

    for declaration in memory {
        let name = match declaration {
            MemoryDeclaration::Scalar { name, .. }
            | MemoryDeclaration::FloatScalar { name, .. }
            | MemoryDeclaration::Buffer { name, .. }
            | MemoryDeclaration::Array { name, .. }
            | MemoryDeclaration::Repeat { name, .. } => name,
        };

        if !names.insert(name) {
            return Err(format!("Memory name {name:?} is already defined"));
        }
    }

    Ok(())
}

fn validate_label_storage_names(
    data: &[DataDeclaration],
    memory: &[MemoryDeclaration],
    labels: &[Label],
) -> Result<(), String> {
    let data_names: HashSet<_> = data
        .iter()
        .flat_map(|declaration| {
            std::iter::once(declaration.name.as_str()).chain(declaration.items.iter().filter_map(
                |item| match item {
                    DataItem::Label { name } => Some(name.as_str()),
                    _ => None,
                },
            ))
        })
        .collect();
    let memory_names: HashSet<_> = memory
        .iter()
        .map(|declaration| match declaration {
            MemoryDeclaration::Scalar { name, .. }
            | MemoryDeclaration::FloatScalar { name, .. }
            | MemoryDeclaration::Buffer { name, .. }
            | MemoryDeclaration::Array { name, .. }
            | MemoryDeclaration::Repeat { name, .. } => name,
        })
        .collect();

    for label in labels {
        let mut names = HashSet::new();

        for instruction in &label.instructions {
            match instruction {
                Instruction::Const { name, .. }
                | Instruction::Stack { name, .. }
                | Instruction::StackString { name, .. } => {
                    if data_names.contains(name.as_str()) {
                        return Err(format!(
                            "Name {name:?} in label {:?} conflicts with top-level data",
                            label.name
                        ));
                    }

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

fn validate_memory_address_targets(
    declaration: &MemoryDeclaration,
    global_symbols: &HashSet<&str>,
) -> Result<(), String> {
    match declaration {
        MemoryDeclaration::Array { name, values, .. } => {
            for value in values {
                validate_memory_address_target(name, value, global_symbols)?;
            }
        }
        MemoryDeclaration::Repeat { name, value, .. } => {
            validate_memory_address_target(name, value, global_symbols)?;
        }
        _ => {}
    }

    Ok(())
}

fn validate_memory_address_target(
    declaration_name: &str,
    value: &MemoryValue,
    global_symbols: &HashSet<&str>,
) -> Result<(), String> {
    if let MemoryValue::Addr { target } = value
        && !global_symbols.contains(target.as_str())
    {
        return Err(format!(
            "Unknown address target {target:?} in memory declaration {declaration_name:?}"
        ));
    }

    Ok(())
}

pub fn validate_program_symbols(program: &Program) -> Result<(), String> {
    let data = &program.data;
    let memory = &program.memory;
    let labels = &program.labels;
    let data_names: HashSet<&str> = data
        .iter()
        .flat_map(|declaration| {
            std::iter::once(declaration.name.as_str()).chain(declaration.items.iter().filter_map(
                |item| match item {
                    DataItem::Label { name } => Some(name.as_str()),
                    _ => None,
                },
            ))
        })
        .collect();
    let memory_names: HashSet<&str> = memory
        .iter()
        .map(|declaration| match declaration {
            MemoryDeclaration::Scalar { name, .. }
            | MemoryDeclaration::FloatScalar { name, .. }
            | MemoryDeclaration::Buffer { name, .. }
            | MemoryDeclaration::Array { name, .. }
            | MemoryDeclaration::Repeat { name, .. } => name.as_str(),
        })
        .collect();
    let mut label_names = HashSet::new();
    let top_level_label_names: HashSet<&str> =
        labels.iter().map(|label| label.name.as_str()).collect();

    for label in labels {
        if data_names.contains(label.name.as_str()) {
            return Err(format!(
                "Label {:?} conflicts with top-level data",
                label.name
            ));
        }

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
                if data_names.contains(name.as_str()) {
                    return Err(format!("Label {:?} conflicts with top-level data", name));
                }

                if memory_names.contains(name.as_str()) {
                    return Err(format!("Label {:?} conflicts with top-level memory", name));
                }

                if !label_names.insert(name.as_str()) {
                    return Err(format!("Label {:?} is already defined", name));
                }
            }
        }
    }

    let global_symbols: HashSet<&str> = data_names
        .iter()
        .copied()
        .chain(memory_names.iter().copied())
        .chain(label_names.iter().copied())
        .collect();

    for declaration in data {
        for item in &declaration.items {
            if let DataItem::Addr { target } = item
                && !global_symbols.contains(target.as_str())
            {
                return Err(format!(
                    "Unknown address target {target:?} in data block {:?}",
                    declaration.name
                ));
            }
        }
    }

    for declaration in memory {
        validate_memory_address_targets(declaration, &global_symbols)?;
    }

    for label in labels {
        let mut bindings = HashSet::new();
        let mut operand_bindings = HashSet::new();
        let mut string_bindings = HashSet::new();
        for instruction in &label.instructions {
            match instruction {
                Instruction::Const { name, value } => {
                    if data_names.contains(name.as_str()) {
                        return Err(format!(
                            "Name {name:?} in label {:?} conflicts with top-level data",
                            label.name
                        ));
                    }

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
                    } else {
                        string_bindings.insert(name.as_str());
                    }
                }
                Instruction::Stack { name, .. } => {
                    if data_names.contains(name.as_str()) {
                        return Err(format!(
                            "Name {name:?} in label {:?} conflicts with top-level data",
                            label.name
                        ));
                    }

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
                    if data_names.contains(name.as_str()) {
                        return Err(format!(
                            "Name {name:?} in label {:?} conflicts with top-level data",
                            label.name
                        ));
                    }

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
                &global_symbols,
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
            if let ControlTarget::Label(target) = target
                && !top_level_labels.contains(target.as_str())
            {
                return Err(format!(
                    "call target {target:?} in label {current_label:?} must be a top-level function"
                ));
            }
        }
        Instruction::Jmp { target, .. } => {
            if let ControlTarget::Label(target) = target {
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

    let mut result = Ok(());
    instruction.visit_operands(|operand| {
        if result.is_err() {
            return;
        }

        if is_string_binding_memory_assignment_operand(instruction, operand, string_bindings) {
            return;
        }

        result = validate_operand_symbol(
            operand,
            bindings,
            operand_bindings,
            string_bindings,
            memory,
            labels,
            current_label,
        );
    });

    result
}

fn is_string_binding_memory_assignment_operand(
    instruction: &Instruction,
    operand: &Operand,
    string_bindings: &HashSet<&str>,
) -> bool {
    let (dst, value) = match instruction {
        Instruction::Assign { dst, value } | Instruction::AssignIf { dst, value, .. } => {
            (dst, value)
        }
        _ => return false,
    };

    if !matches!(dst, AssignmentTarget::Operand(Operand::Dereference { .. })) {
        return false;
    }

    let AssignmentValue::Operand(Operand::Ident(name)) = value else {
        return false;
    };

    operand == &Operand::Ident(name.clone()) && string_bindings.contains(name.as_str())
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
        Operand::Converted { operand, .. } | Operand::Cast { operand, .. } => {
            validate_operand_symbol(
                operand,
                bindings,
                operand_bindings,
                string_bindings,
                memory,
                labels,
                current_label,
            )
        }
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
            format!("Binding {name:?} in label {current_label:?} is not a string"),
        ),
        Operand::Pointer(name)
            if !memory.contains(name.as_str()) && !labels.contains(name.as_str()) =>
        {
            Err(format!(
                "Unknown address target {name:?} in label {current_label:?}"
            ))
        }
        Operand::Dereference { address, .. } | Operand::AddressOf(address) => {
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

enum FormatSegment {
    Literal(String),
    Placeholder(Option<PrintFormat>),
}

enum PrintArg {
    Ident(String),
    Operand(Operand),
}

fn print_part_for_format_arg(format: Option<PrintFormat>, arg: PrintArg) -> PrintPart {
    match (format, arg) {
        (None, PrintArg::Ident(name)) => PrintPart::Binding(name),
        (None, PrintArg::Operand(operand)) => PrintPart::FormattedOperand {
            format: PrintFormat::Infer,
            operand,
        },
        (Some(format), PrintArg::Ident(name)) => PrintPart::FormattedOperand {
            format,
            operand: Operand::Ident(name),
        },
        (Some(format), PrintArg::Operand(operand)) => {
            PrintPart::FormattedOperand { format, operand }
        }
    }
}

fn split_format_literal(value: &str) -> Result<Vec<FormatSegment>, String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars().peekable();

    while let Some(char) = chars.next() {
        match char {
            '{' => {
                segments.push(FormatSegment::Literal(current));
                current = String::new();

                let mut specifier = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some('{') => {
                            return Err(String::from("Nested '{' in print format placeholder"));
                        }
                        Some(char) => specifier.push(char),
                        None => return Err(String::from("Unclosed '{' in print format string")),
                    }
                }

                let format = match specifier.as_str() {
                    "" => None,
                    "i8" => Some(PrintFormat::SignedDecimal(MemoryWidth::I8)),
                    "i16" => Some(PrintFormat::SignedDecimal(MemoryWidth::I16)),
                    "i32" => Some(PrintFormat::SignedDecimal(MemoryWidth::I32)),
                    "i64" => Some(PrintFormat::SignedDecimal(MemoryWidth::I64)),
                    "u8" => Some(PrintFormat::UnsignedDecimal(MemoryWidth::U8)),
                    "u16" => Some(PrintFormat::UnsignedDecimal(MemoryWidth::U16)),
                    "u32" => Some(PrintFormat::UnsignedDecimal(MemoryWidth::U32)),
                    "u64" => Some(PrintFormat::UnsignedDecimal(MemoryWidth::U64)),
                    "x" => Some(PrintFormat::Hex),
                    "b" => Some(PrintFormat::Binary),
                    "ptr" => Some(PrintFormat::Pointer),
                    _ => {
                        return Err(format!(
                            "Unknown print format {{{specifier}}}; expected {{}}, {{i8}}, {{i16}}, {{i32}}, {{i64}}, {{u8}}, {{u16}}, {{u32}}, {{u64}}, {{x}}, {{b}}, or {{ptr}}"
                        ));
                    }
                };

                segments.push(FormatSegment::Placeholder(format));
            }
            '}' => return Err(String::from("Unmatched '}' in print format string")),
            _ => current.push(char),
        }
    }

    segments.push(FormatSegment::Literal(current));

    Ok(segments)
}

fn is_register_name(s: &str) -> bool {
    crate::register::is_register(s)
}

fn is_xmm_register_name(s: &str) -> bool {
    crate::register::is_xmm(s)
}
