use crate::analysis::{
    FloatBinding, ImmediateDestination, StackFrame, StackSlot, StringBinding, StringTable, Width,
    build_stack_frame, collect_string_bindings, destination_width, float_memory_width,
    immediate_value, is_float_memory_operand, memory_width_bits, operand_width, register_width,
    resolve_memory_width, stack_scalar_slot, stack_string_property_slot, stack_string_slot,
    validate_float_literal, validate_float_width, validate_label,
};
use crate::ast::{
    Address, AddressOperator, AddressTerm, AssignmentTarget, AssignmentValue, BitwiseUnaryOp,
    CompareOp, Condition, ConditionExpr, ControlTarget, DataDeclaration, DataItem, ExprOp,
    Expression, FloatMathOp, Instruction, IntrinsicOp, MathOp, MemoryDeclaration, MemoryValue,
    MemoryWidth, Operand, PairBinaryOp, PrintFormat, PrintPart, Program, ReadSource, RegisterPair,
    StringInitializer, StringProperty, WidthConversion,
};
use crate::diagnostic::{Diagnostic, ProgramOrigins};
use crate::parser::validate_program_symbols;
use std::collections::{HashMap, HashSet};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Target {
    X86_64,
    X86_64Free,
}

struct LabelSymbols<'a> {
    source_entry: &'a str,
    entry_symbol: &'a str,
}

impl<'a> LabelSymbols<'a> {
    fn emit_label(&self, source_label: &str) -> String {
        if source_label == self.source_entry {
            self.entry_symbol.to_string()
        } else {
            source_label.to_string()
        }
    }
}

impl Target {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "x86_64" => Ok(Self::X86_64),
            "x86_64-free" => Ok(Self::X86_64Free),
            _ => Err(format!(
                "Unknown target {name:?}; expected x86_64 or x86_64-free"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::X86_64Free => "x86_64-free",
        }
    }

    fn is_freestanding(self) -> bool {
        matches!(self, Self::X86_64Free)
    }
}

pub fn emit_x86_64_linux_asm(program: &Program) -> Result<String, String> {
    emit_x86_64_asm(program, Target::X86_64)
}

/// Runs semantic validation with the best available source location while the
/// public AST remains span-free.
pub fn validate_program_with_diagnostics(
    program: &Program,
    origins: &ProgramOrigins,
) -> Result<(), Diagnostic> {
    validate_program_symbols(program).map_err(Diagnostic::new)?;

    let top_level_labels: HashSet<&str> = program
        .labels
        .iter()
        .map(|label| label.name.as_str())
        .collect();
    for label in &program.labels {
        let stack = build_stack_frame(label);
        validate_label(label, &top_level_labels, &stack).map_err(|message| {
            let diagnostic = Diagnostic::new(message);
            origins
                .instruction_span(&label.name, 0)
                .map_or(diagnostic.clone(), |span| diagnostic.at(span))
        })?;
    }

    Ok(())
}

pub fn emit_x86_64_asm(program: &Program, target: Target) -> Result<String, String> {
    emit_x86_64_asm_with_entry_symbol(program, target, "_start")
}

pub fn emit_x86_64_asm_with_entry_symbol(
    program: &Program,
    target: Target,
    entry_symbol: &str,
) -> Result<String, String> {
    emit_x86_64_asm_impl(program, target, entry_symbol, None)
}

pub fn emit_x86_64_asm_with_origins(
    program: &Program,
    target: Target,
    entry_symbol: &str,
    origins: &ProgramOrigins,
) -> Result<String, Diagnostic> {
    validate_program_with_diagnostics(program, origins)?;
    emit_x86_64_asm_impl(program, target, entry_symbol, Some(origins)).map_err(|message| {
        let Some((label, index, message)) =
            message
                .strip_prefix("__SUBSEA_CODEGEN__")
                .and_then(|value| {
                    let mut parts = value.splitn(3, '\0');
                    Some((parts.next()?, parts.next()?.parse().ok()?, parts.next()?))
                })
        else {
            return Diagnostic::new(message);
        };
        let diagnostic = Diagnostic::new(message.to_owned());
        origins
            .instruction_span(label, index)
            .map_or(diagnostic.clone(), |span| diagnostic.at(span))
    })
}

fn emit_x86_64_asm_impl(
    program: &Program,
    target: Target,
    entry_symbol: &str,
    origins: Option<&ProgramOrigins>,
) -> Result<String, String> {
    let strings = collect_string_bindings(program)?;
    let mut literal_indexes = HashMap::new();
    let mut asm = String::new();
    let labels = LabelSymbols {
        source_entry: &program.entry,
        entry_symbol,
    };

    asm.push_str(".intel_syntax noprefix\n");
    emit_static_data(&mut asm, &program.data, &labels);
    emit_data(&mut asm, &program.memory, &labels);
    emit_bss(&mut asm, &program.memory);
    emit_rodata(&mut asm, &strings.all, &strings.floats);
    asm.push_str(".section .text\n");
    asm.push_str(&format!(".global {entry_symbol}\n\n"));

    let top_level_labels: HashSet<&str> = program
        .labels
        .iter()
        .map(|label| label.name.as_str())
        .collect();

    for label in &program.labels {
        let stack = build_stack_frame(label);

        asm.push_str(&format!("{}:\n", labels.emit_label(&label.name)));

        if stack.has_slots() {
            emit_frame_prologue(&mut asm, &stack);
            emit_stack_initializers(&mut asm, &label.instructions, &strings, &label.name, &stack)?;
        }

        let mut runtime_print_index = 0;
        let mut conditional_jump_index = 0;

        for (instruction_index, instruction) in label.instructions.iter().enumerate() {
            let result: Result<(), String> = (|| {
                match instruction {
                    Instruction::Assign { dst, value } => {
                        if target.is_freestanding() && assignment_value_uses_linux_reserve(value) {
                            return Err(String::from(
                                "reserve is only supported for target x86_64",
                            ));
                        }

                        emit_assignment(&mut asm, dst, value, &strings, &label.name, &stack)?;
                    }
                    Instruction::AssignIf {
                        dst,
                        value,
                        condition,
                    } => {
                        if target.is_freestanding() && assignment_value_uses_linux_reserve(value) {
                            return Err(String::from(
                                "reserve is only supported for target x86_64",
                            ));
                        }

                        conditional_jump_index += 1;
                        let skip_label = format!(
                            ".L.__subsea.{}.assign_if_{}_skip",
                            label.name, conditional_jump_index
                        );
                        emit_condition_jump(
                            &mut asm,
                            &skip_label,
                            condition,
                            false,
                            &strings,
                            &label.name,
                            &stack,
                            conditional_jump_index,
                        )?;
                        emit_assignment(&mut asm, dst, value, &strings, &label.name, &stack)?;
                        asm.push_str(&format!("{skip_label}:\n"));
                    }
                    Instruction::Call { target } => {
                        emit_call_instruction(
                            &mut asm,
                            target,
                            &labels,
                            &strings,
                            &label.name,
                            &stack,
                        )?;
                    }
                    Instruction::Exit { code } => {
                        if target.is_freestanding() {
                            return Err(String::from(
                                "exit is only supported for target x86_64; use hlt or an explicit loop for x86_64-free",
                            ));
                        }

                        asm.push_str("  mov rax, 60\n");
                        asm.push_str(&format!("  mov rdi, {code}\n"));
                        asm.push_str("  syscall\n");
                    }
                    Instruction::InlineAsm { text } => {
                        asm.push_str(&format!("  {text}\n"));
                    }
                    Instruction::Jmp { target, condition } => {
                        conditional_jump_index += usize::from(condition.is_some());
                        emit_jmp_instruction(
                            &mut asm,
                            target,
                            condition.as_ref(),
                            &labels,
                            &strings,
                            &label.name,
                            &stack,
                            conditional_jump_index,
                        )?;
                    }
                    Instruction::Label { name } => {
                        asm.push_str(&format!("{name}:\n"));
                    }
                    Instruction::Nop => {
                        asm.push_str("  nop\n");
                    }
                    Instruction::Const { .. } | Instruction::Stack { .. } => {}
                    Instruction::StackString { name, value } => {
                        emit_stack_string_initializer(
                            &mut asm,
                            name,
                            value,
                            &strings,
                            &label.name,
                            &stack,
                        )?;
                    }
                    Instruction::Print { parts } => {
                        if target.is_freestanding() {
                            return Err(String::from("print is only supported for target x86_64"));
                        }

                        for part in parts {
                            match part {
                                PrintPart::Binding(name) => {
                                    if let Some(slot) = stack.slots.get(name) {
                                        runtime_print_index += 1;
                                        match slot {
                                            StackSlot::Scalar { width, .. } => {
                                                let format = infer_print_format_for_width(*width);
                                                emit_print_operand_instruction(
                                                    &mut asm,
                                                    &Operand::Ident(name.clone()),
                                                    format,
                                                    &strings,
                                                    &label.name,
                                                    &stack,
                                                    runtime_print_index,
                                                )?;
                                            }
                                            StackSlot::String { .. } => {
                                                emit_print_stack_string_instruction(
                                                    &mut asm, name, &stack,
                                                )?;
                                            }
                                        }
                                    } else {
                                        let string = resolve_print_part(
                                            &strings,
                                            &mut literal_indexes,
                                            &label.name,
                                            part,
                                        )?;

                                        emit_print_string_instruction(&mut asm, string);
                                    }
                                }
                                PrintPart::Literal(_) => {
                                    let string = resolve_print_part(
                                        &strings,
                                        &mut literal_indexes,
                                        &label.name,
                                        part,
                                    )?;

                                    emit_print_string_instruction(&mut asm, string);
                                }
                                PrintPart::Operand(operand) => {
                                    runtime_print_index += 1;
                                    emit_print_operand_instruction(
                                        &mut asm,
                                        operand,
                                        PrintFormat::SignedDecimal(MemoryWidth::I64),
                                        &strings,
                                        &label.name,
                                        &stack,
                                        runtime_print_index,
                                    )?;
                                }
                                PrintPart::FormattedOperand { format, operand } => {
                                    runtime_print_index += 1;
                                    emit_print_operand_instruction(
                                        &mut asm,
                                        operand,
                                        *format,
                                        &strings,
                                        &label.name,
                                        &stack,
                                        runtime_print_index,
                                    )?;
                                }
                            }
                        }
                    }
                    Instruction::Pop { dst } => {
                        validate_pop_operand(dst, &strings, &stack)?;
                        let dst = emit_operand(dst, &strings, &label.name, &stack)?;
                        asm.push_str(&format!("  pop {dst}\n"));
                    }
                    Instruction::Push { src } => {
                        validate_push_operand(src, &strings, &label.name, &stack)?;
                        let src = emit_operand(src, &strings, &label.name, &stack)?;
                        asm.push_str(&format!("  push {src}\n"));
                    }
                    Instruction::Read { src, dst, len } => {
                        if target.is_freestanding() {
                            return Err(String::from("read is only supported for target x86_64"));
                        }

                        emit_read_instruction(
                            &mut asm,
                            src,
                            dst,
                            len,
                            &strings,
                            &label.name,
                            &stack,
                        )?;
                    }
                    Instruction::Release { ptr, len } => {
                        if target.is_freestanding() {
                            return Err(String::from(
                                "release is only supported for target x86_64",
                            ));
                        }

                        emit_release_instruction(
                            &mut asm,
                            ptr,
                            len,
                            &strings,
                            &label.name,
                            &stack,
                        )?;
                    }
                    Instruction::Ret => {
                        if stack.has_slots() {
                            emit_frame_epilogue(&mut asm);
                        }
                        asm.push_str("  ret\n");
                    }
                    Instruction::Syscall => {
                        asm.push_str("  syscall\n");
                    }
                }
                Ok(())
            })();
            if let Err(message) = result {
                return Err(match origins {
                    Some(_) => format!(
                        "__SUBSEA_CODEGEN__{}\0{}\0{}",
                        label.name, instruction_index, message
                    ),
                    None => message,
                });
            }
        }

        validate_label(label, &top_level_labels, &stack)?;

        asm.push('\n');
    }

    Ok(asm)
}

fn emit_conditional_jump(
    asm: &mut String,
    target: &str,
    condition: &ConditionExpr,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    index: usize,
) -> Result<(), String> {
    emit_condition_jump(
        asm, target, condition, true, strings, label_name, stack, index,
    )
}

fn emit_condition_jump(
    asm: &mut String,
    target: &str,
    condition: &ConditionExpr,
    jump_if_true: bool,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    index: usize,
) -> Result<(), String> {
    match condition {
        ConditionExpr::Compare(condition) => emit_compare_condition_jump(
            asm,
            target,
            condition,
            jump_if_true,
            strings,
            label_name,
            stack,
            index,
        ),
        ConditionExpr::BitwiseAndZero { lhs, rhs, op } => emit_test_condition_jump(
            asm,
            target,
            lhs,
            rhs,
            *op,
            jump_if_true,
            strings,
            label_name,
            stack,
        ),
    }
}

fn emit_call_instruction(
    asm: &mut String,
    target: &ControlTarget,
    labels: &LabelSymbols,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match target {
        ControlTarget::Label(target) => {
            asm.push_str(&format!("  call {}\n", labels.emit_label(target)));
            Ok(())
        }
        ControlTarget::Operand(operand) => {
            validate_indirect_control_target("call", operand, strings, label_name, stack)?;
            let operand = emit_operand(operand, strings, label_name, stack)?;
            asm.push_str(&format!("  call {operand}\n"));
            Ok(())
        }
    }
}

fn emit_jmp_instruction(
    asm: &mut String,
    target: &ControlTarget,
    condition: Option<&ConditionExpr>,
    labels: &LabelSymbols,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    index: usize,
) -> Result<(), String> {
    match target {
        ControlTarget::Label(target) => {
            if let Some(condition) = condition {
                emit_conditional_jump(
                    asm,
                    &labels.emit_label(target),
                    condition,
                    strings,
                    label_name,
                    stack,
                    index,
                )?;
            } else {
                asm.push_str(&format!("  jmp {}\n", labels.emit_label(target)));
            }

            Ok(())
        }
        ControlTarget::Operand(operand) => {
            validate_indirect_control_target("jmp", operand, strings, label_name, stack)?;
            let operand = emit_operand(operand, strings, label_name, stack)?;

            if let Some(condition) = condition {
                let skip_label = format!(".L.__subsea.{label_name}.indirect_jmp_{index}_skip");
                emit_condition_jump(
                    asm,
                    &skip_label,
                    condition,
                    false,
                    strings,
                    label_name,
                    stack,
                    index,
                )?;
                asm.push_str(&format!("  jmp {operand}\n"));
                asm.push_str(&format!("{skip_label}:\n"));
            } else {
                asm.push_str(&format!("  jmp {operand}\n"));
            }

            Ok(())
        }
    }
}

fn emit_compare_condition_jump(
    asm: &mut String,
    target: &str,
    condition: &Condition,
    jump_if_true: bool,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    index: usize,
) -> Result<(), String> {
    if let Some(width) = resolve_float_compare_width(condition, strings, label_name, stack)? {
        return emit_float_conditional_jump(
            asm,
            target,
            condition,
            width,
            jump_if_true,
            strings,
            label_name,
            stack,
            index,
        );
    }

    let (lhs, rhs, op) = normalize_compare(
        &condition.lhs,
        &condition.rhs,
        condition.op,
        strings,
        label_name,
        stack,
    )?;

    validate_resolved_integer_compare_op(op)?;
    validate_compare_operands(lhs, rhs, strings, label_name, stack)?;

    let use_test = matches!(op, CompareOp::Equal | CompareOp::NotEqual)
        && matches!(lhs, Operand::Register(register) if !is_xmm_register(register))
        && immediate_value(rhs, strings, label_name, stack) == Some(0);
    let lhs = emit_operand(lhs, strings, label_name, stack)?;
    let rhs = emit_operand(rhs, strings, label_name, stack)?;
    let op = if jump_if_true {
        op
    } else {
        invert_compare_op(op)
    };
    if use_test {
        asm.push_str(&format!("  test {lhs}, {lhs}\n"));
    } else {
        asm.push_str(&format!("  cmp {lhs}, {rhs}\n"));
    }
    asm.push_str(&format!("  {} {target}\n", compare_jump_opcode(op)));

    Ok(())
}

fn emit_test_condition_jump(
    asm: &mut String,
    target: &str,
    lhs: &Operand,
    rhs: &Operand,
    op: CompareOp,
    jump_if_true: bool,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_test_condition_operands(lhs, rhs, op, strings, label_name, stack)?;

    let lhs = emit_operand(lhs, strings, label_name, stack)?;
    let rhs = emit_operand(rhs, strings, label_name, stack)?;
    let jump = match (op, jump_if_true) {
        (CompareOp::Equal, true) | (CompareOp::NotEqual, false) => "je",
        (CompareOp::NotEqual, true) | (CompareOp::Equal, false) => "jne",
        _ => unreachable!(),
    };

    asm.push_str(&format!("  test {lhs}, {rhs}\n"));
    asm.push_str(&format!("  {jump} {target}\n"));

    Ok(())
}

fn normalize_compare<'a>(
    lhs: &'a Operand,
    rhs: &'a Operand,
    op: CompareOp,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(&'a Operand, &'a Operand, CompareOp), String> {
    if is_immediate_operand(lhs, strings, label_name, stack) {
        if is_immediate_operand(rhs, strings, label_name, stack) {
            return Err(String::from("Comparison cannot use two immediate operands"));
        }

        Ok((rhs, lhs, reverse_compare_op(op)))
    } else {
        Ok((lhs, rhs, op))
    }
}

fn reverse_compare_op(op: CompareOp) -> CompareOp {
    match op {
        CompareOp::Equal => CompareOp::Equal,
        CompareOp::NotEqual => CompareOp::NotEqual,
        CompareOp::Less => CompareOp::Greater,
        CompareOp::LessEqual => CompareOp::GreaterEqual,
        CompareOp::Greater => CompareOp::Less,
        CompareOp::GreaterEqual => CompareOp::LessEqual,
        CompareOp::SignedLess => CompareOp::SignedGreater,
        CompareOp::SignedLessEqual => CompareOp::SignedGreaterEqual,
        CompareOp::SignedGreater => CompareOp::SignedLess,
        CompareOp::SignedGreaterEqual => CompareOp::SignedLessEqual,
        CompareOp::UnsignedLess => CompareOp::UnsignedGreater,
        CompareOp::UnsignedLessEqual => CompareOp::UnsignedGreaterEqual,
        CompareOp::UnsignedGreater => CompareOp::UnsignedLess,
        CompareOp::UnsignedGreaterEqual => CompareOp::UnsignedLessEqual,
        CompareOp::FloatEqual(_)
        | CompareOp::FloatNotEqual(_)
        | CompareOp::FloatLess(_)
        | CompareOp::FloatLessEqual(_)
        | CompareOp::FloatGreater(_)
        | CompareOp::FloatGreaterEqual(_) => op,
    }
}

fn invert_compare_op(op: CompareOp) -> CompareOp {
    match op {
        CompareOp::Equal => CompareOp::NotEqual,
        CompareOp::NotEqual => CompareOp::Equal,
        CompareOp::Less => CompareOp::GreaterEqual,
        CompareOp::LessEqual => CompareOp::Greater,
        CompareOp::Greater => CompareOp::LessEqual,
        CompareOp::GreaterEqual => CompareOp::Less,
        CompareOp::SignedLess => CompareOp::SignedGreaterEqual,
        CompareOp::SignedLessEqual => CompareOp::SignedGreater,
        CompareOp::SignedGreater => CompareOp::SignedLessEqual,
        CompareOp::SignedGreaterEqual => CompareOp::SignedLess,
        CompareOp::UnsignedLess => CompareOp::UnsignedGreaterEqual,
        CompareOp::UnsignedLessEqual => CompareOp::UnsignedGreater,
        CompareOp::UnsignedGreater => CompareOp::UnsignedLessEqual,
        CompareOp::UnsignedGreaterEqual => CompareOp::UnsignedLess,
        CompareOp::FloatEqual(width) => CompareOp::FloatNotEqual(width),
        CompareOp::FloatNotEqual(width) => CompareOp::FloatEqual(width),
        CompareOp::FloatLess(width) => CompareOp::FloatGreaterEqual(width),
        CompareOp::FloatLessEqual(width) => CompareOp::FloatGreater(width),
        CompareOp::FloatGreater(width) => CompareOp::FloatLessEqual(width),
        CompareOp::FloatGreaterEqual(width) => CompareOp::FloatLess(width),
    }
}

fn compare_jump_opcode(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Equal => "je",
        CompareOp::NotEqual => "jne",
        CompareOp::Less | CompareOp::LessEqual | CompareOp::Greater | CompareOp::GreaterEqual => {
            unreachable!()
        }
        CompareOp::SignedLess => "jl",
        CompareOp::SignedLessEqual => "jle",
        CompareOp::SignedGreater => "jg",
        CompareOp::SignedGreaterEqual => "jge",
        CompareOp::UnsignedLess => "jb",
        CompareOp::UnsignedLessEqual => "jbe",
        CompareOp::UnsignedGreater => "ja",
        CompareOp::UnsignedGreaterEqual => "jae",
        CompareOp::FloatEqual(_)
        | CompareOp::FloatNotEqual(_)
        | CompareOp::FloatLess(_)
        | CompareOp::FloatLessEqual(_)
        | CompareOp::FloatGreater(_)
        | CompareOp::FloatGreaterEqual(_) => unreachable!(),
    }
}

fn validate_resolved_integer_compare_op(op: CompareOp) -> Result<(), String> {
    match op {
        CompareOp::Less => Err(String::from(
            "Comparison '<' must specify signedness; use i< or u<",
        )),
        CompareOp::LessEqual => Err(String::from(
            "Comparison '<=' must specify signedness; use i<= or u<=",
        )),
        CompareOp::Greater => Err(String::from(
            "Comparison '>' must specify signedness; use i> or u>",
        )),
        CompareOp::GreaterEqual => Err(String::from(
            "Comparison '>=' must specify signedness; use i>= or u>=",
        )),
        _ => Ok(()),
    }
}

fn float_compare_width(op: CompareOp) -> Option<MemoryWidth> {
    match op {
        CompareOp::FloatEqual(width)
        | CompareOp::FloatNotEqual(width)
        | CompareOp::FloatLess(width)
        | CompareOp::FloatLessEqual(width)
        | CompareOp::FloatGreater(width)
        | CompareOp::FloatGreaterEqual(width) => Some(width),
        _ => None,
    }
}

fn resolve_float_compare_width(
    condition: &Condition,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<Option<MemoryWidth>, String> {
    if let Some(width) = float_compare_width(condition.op) {
        return Ok(Some(width));
    }

    if !matches!(
        condition.op,
        CompareOp::Equal
            | CompareOp::NotEqual
            | CompareOp::Less
            | CompareOp::LessEqual
            | CompareOp::Greater
            | CompareOp::GreaterEqual
    ) {
        return Ok(None);
    }

    resolve_float_pair_width(
        &condition.lhs,
        &condition.rhs,
        strings,
        label_name,
        stack,
        "Floating-point comparison operands must have matching widths",
    )
}

fn operand_float_width(
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<Option<MemoryWidth>, String> {
    match operand {
        Operand::Dereference { address, width } => {
            Ok(resolve_memory_width(address, *width, strings)?.filter(|width| width.is_float()))
        }
        Operand::Ident(name) => {
            if let Some((_, width)) = stack_scalar_slot(stack, name) {
                Ok(width.is_float().then_some(width))
            } else {
                Ok(strings
                    .float_bindings
                    .get(&(label_name.to_string(), name.clone()))
                    .map(|binding| binding.width))
            }
        }
        _ => Ok(None),
    }
}

fn can_use_float_context(operand: &Operand) -> bool {
    matches!(
        operand,
        Operand::Register(_)
            | Operand::FloatLiteral(_)
            | Operand::Dereference { .. }
            | Operand::Ident(_)
    )
}

fn resolve_float_binary_width(
    lhs: &Operand,
    rhs: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<Option<MemoryWidth>, String> {
    resolve_float_pair_width(
        lhs,
        rhs,
        strings,
        label_name,
        stack,
        "Floating-point arithmetic operands must have matching widths",
    )
}

fn resolve_float_pair_width(
    lhs: &Operand,
    rhs: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    mismatch_error: &str,
) -> Result<Option<MemoryWidth>, String> {
    let lhs_width = operand_float_width(lhs, strings, label_name, stack)?;
    let rhs_width = operand_float_width(rhs, strings, label_name, stack)?;

    match (lhs_width, rhs_width) {
        (Some(left), Some(right)) if left == right => Ok(Some(left)),
        (Some(left), None) if can_use_float_context(rhs) => Ok(Some(left)),
        (None, Some(right)) if can_use_float_context(lhs) => Ok(Some(right)),
        (Some(_), Some(_)) => Err(String::from(mismatch_error)),
        _ => Ok(None),
    }
}

fn float_math_op_from_integer_op(op: MathOp) -> FloatMathOp {
    match op {
        MathOp::Add => FloatMathOp::Add,
        MathOp::Multiply => FloatMathOp::Multiply,
        MathOp::Subtract => FloatMathOp::Subtract,
        MathOp::Power
        | MathOp::BitAnd
        | MathOp::BitOr
        | MathOp::BitXor
        | MathOp::ShiftLeft
        | MathOp::ShiftRightArithmetic
        | MathOp::ShiftRightLogical => unreachable!(),
    }
}

fn is_ambiguous_float_binary_operand(operand: &Operand) -> bool {
    matches!(operand, Operand::FloatLiteral(_))
        || matches!(operand, Operand::Register(register) if is_xmm_register(register))
}

fn math_op_symbol(op: MathOp) -> &'static str {
    match op {
        MathOp::Add => "+",
        MathOp::BitAnd => "&",
        MathOp::BitOr => "|",
        MathOp::BitXor => "^",
        MathOp::Multiply => "*",
        MathOp::Power => "**",
        MathOp::ShiftLeft => "<<",
        MathOp::ShiftRightArithmetic => "i>>",
        MathOp::ShiftRightLogical => ">>",
        MathOp::Subtract => "-",
    }
}

fn integer_op_can_be_float(op: MathOp) -> bool {
    matches!(op, MathOp::Add | MathOp::Multiply | MathOp::Subtract)
}

fn is_commutative_math_op(op: MathOp) -> bool {
    matches!(
        op,
        MathOp::Add | MathOp::Multiply | MathOp::BitAnd | MathOp::BitOr | MathOp::BitXor
    )
}

fn is_shift_math_op(op: MathOp) -> bool {
    matches!(
        op,
        MathOp::ShiftLeft | MathOp::ShiftRightArithmetic | MathOp::ShiftRightLogical
    )
}

fn integer_math_opcode(op: MathOp) -> &'static str {
    match op {
        MathOp::Add => "add",
        MathOp::BitAnd => "and",
        MathOp::BitOr => "or",
        MathOp::BitXor => "xor",
        MathOp::Multiply => "imul",
        MathOp::Power => unreachable!(),
        MathOp::ShiftLeft => "shl",
        MathOp::ShiftRightArithmetic => "sar",
        MathOp::ShiftRightLogical => "shr",
        MathOp::Subtract => "sub",
    }
}

fn emit_float_conditional_jump(
    asm: &mut String,
    target: &str,
    condition: &Condition,
    width: MemoryWidth,
    jump_if_true: bool,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    index: usize,
) -> Result<(), String> {
    validate_float_math_operand(
        "Floating-point comparison left operand",
        &condition.lhs,
        width,
        strings,
        label_name,
        stack,
    )?;
    validate_float_math_operand(
        "Floating-point comparison right operand",
        &condition.rhs,
        width,
        strings,
        label_name,
        stack,
    )?;

    if is_memory_operand(&condition.lhs, stack) && is_memory_operand(&condition.rhs, stack) {
        return Err(String::from(
            "Floating-point comparison cannot use memory for both operands",
        ));
    }

    let lhs = emit_float_operand(&condition.lhs, width, strings, label_name, stack)?;
    let rhs = emit_float_operand(&condition.rhs, width, strings, label_name, stack)?;
    let ordered_label = format!(".L.__subsea.{label_name}.fcmp_{index}_ordered");

    asm.push_str(&format!("  {} {lhs}, {rhs}\n", float_compare_opcode(width)));
    if jump_if_true {
        asm.push_str(&format!("  jp {ordered_label}\n"));
    } else {
        asm.push_str(&format!("  jp {target}\n"));
    }
    let op = if jump_if_true {
        condition.op
    } else {
        invert_compare_op(condition.op)
    };
    asm.push_str(&format!("  {} {target}\n", float_compare_jump_opcode(op)));
    if jump_if_true {
        asm.push_str(&format!("{ordered_label}:\n"));
    }

    Ok(())
}

fn float_compare_opcode(width: MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::F32 => "ucomiss",
        MemoryWidth::F64 => "ucomisd",
        _ => unreachable!(),
    }
}

fn float_compare_jump_opcode(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Equal | CompareOp::FloatEqual(_) => "je",
        CompareOp::NotEqual | CompareOp::FloatNotEqual(_) => "jne",
        CompareOp::Less | CompareOp::FloatLess(_) => "jb",
        CompareOp::LessEqual | CompareOp::FloatLessEqual(_) => "jbe",
        CompareOp::Greater | CompareOp::FloatGreater(_) => "ja",
        CompareOp::GreaterEqual | CompareOp::FloatGreaterEqual(_) => "jae",
        _ => unreachable!(),
    }
}

fn validate_compare_operands(
    lhs: &Operand,
    rhs: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if operand_uses_xmm_register(lhs)
        || operand_uses_xmm_register(rhs)
        || is_float_memory_operand(lhs, strings, stack)?
        || is_float_memory_operand(rhs, strings, stack)?
    {
        return Err(String::from(
            "Floating-point operands cannot be compared yet",
        ));
    }

    if matches!(lhs, Operand::Pointer(_)) || matches!(rhs, Operand::Pointer(_)) {
        return Err(String::from("Comparison cannot use an address-of operand"));
    }

    if is_memory_operand(lhs, stack) && is_memory_operand(rhs, stack) {
        return Err(String::from(
            "Comparison cannot use memory for both operands",
        ));
    }

    if let (Some(lhs_width), Some(rhs_width)) = (
        operand_width(lhs, strings, label_name, stack)?,
        operand_width(rhs, strings, label_name, stack)?,
    ) && lhs_width != rhs_width
    {
        return Err(format!(
            "Cannot compare {}-bit operand with {}-bit operand",
            lhs_width.bits(),
            rhs_width.bits()
        ));
    }

    if let (Some(value), Some(width)) = (
        immediate_value(rhs, strings, label_name, stack),
        destination_width(lhs, strings, stack)?,
    ) {
        validate_immediate_range(value, width)?;
    }

    if is_immediate_operand(rhs, strings, label_name, stack)
        && matches!(lhs, Operand::Dereference { width: None, .. })
    {
        return Err(String::from(
            "Cannot compare an immediate value with memory without an explicit width",
        ));
    }

    Ok(())
}

fn validate_test_condition_operands(
    lhs: &Operand,
    rhs: &Operand,
    op: CompareOp,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if !matches!(op, CompareOp::Equal | CompareOp::NotEqual) {
        return Err(String::from(
            "Bitwise-and conditions only support == 0 or != 0",
        ));
    }

    validate_binary_operands("test", rhs, lhs, strings, label_name, stack)
}

fn emit_frame_prologue(asm: &mut String, stack: &StackFrame) {
    asm.push_str("  push rbp\n");
    asm.push_str("  mov rbp, rsp\n");
    if stack.size > 0 {
        asm.push_str(&format!("  sub rsp, {}\n", stack.size));
    }
}

fn emit_frame_epilogue(asm: &mut String) {
    asm.push_str("  mov rsp, rbp\n");
    asm.push_str("  pop rbp\n");
}

fn emit_stack_initializers(
    asm: &mut String,
    instructions: &[Instruction],
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    for instruction in instructions {
        match instruction {
            Instruction::Stack { name, width, value } if width.is_float() => {
                emit_stack_float_initializer(asm, name, *width, value, strings, label_name, stack)?;
            }
            Instruction::Stack { name, value, .. } => {
                if !is_immediate_operand(value, strings, label_name, stack) {
                    return Err(format!(
                        "Stack variable {name:?} initializer must be an integer immediate or const"
                    ));
                }

                let dst = Operand::Ident(name.clone());
                emit_copy_instruction(asm, value, &dst, strings, label_name, stack)?;
            }
            Instruction::StackString { .. } => {}
            _ => {}
        }
    }

    Ok(())
}

fn emit_stack_float_initializer(
    asm: &mut String,
    name: &str,
    width: MemoryWidth,
    value: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_float_math_operand(
        "Floating-point stack initializer",
        value,
        width,
        strings,
        label_name,
        stack,
    )?;

    let (offset, _) =
        stack_scalar_slot(stack, name).ok_or_else(|| format!("Unknown stack variable {name:?}"))?;
    let src = emit_float_operand(value, width, strings, label_name, stack)?;
    let ptr = width.ptr();

    asm.push_str("  push rax\n");
    match width {
        MemoryWidth::F32 => {
            asm.push_str(&format!("  mov eax, {src}\n"));
            asm.push_str(&format!("  mov {ptr} ptr [rbp - {offset}], eax\n"));
        }
        MemoryWidth::F64 => {
            asm.push_str(&format!("  mov rax, {src}\n"));
            asm.push_str(&format!("  mov {ptr} ptr [rbp - {offset}], rax\n"));
        }
        _ => unreachable!(),
    }
    asm.push_str("  pop rax\n");

    Ok(())
}

fn operand_is_stack_slot(operand: &Operand, stack: &StackFrame) -> bool {
    matches!(operand, Operand::Ident(name) if stack_scalar_slot(stack, name).is_some())
}

fn is_memory_operand(operand: &Operand, stack: &StackFrame) -> bool {
    matches!(
        operand,
        Operand::Dereference { .. } | Operand::StringProperty { .. }
    ) || operand_is_stack_slot(operand, stack)
}

fn validate_indirect_control_target(
    instruction: &str,
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(
        operand,
        Operand::Immediate(_)
            | Operand::Pointer(_)
            | Operand::AddressOf(_)
            | Operand::StringProperty { .. }
            | Operand::Converted { .. }
            | Operand::Cast { .. }
            | Operand::FloatLiteral(_)
    ) || matches!(operand, Operand::Ident(name) if stack_scalar_slot(stack, name).is_none())
    {
        return Err(format!(
            "indirect {instruction} target must be a 64-bit register or memory operand"
        ));
    }

    if operand_uses_xmm_register(operand) || is_float_memory_operand(operand, strings, stack)? {
        return Err(format!(
            "indirect {instruction} target must be a 64-bit integer register or memory operand"
        ));
    }

    match operand_width(operand, strings, label_name, stack)? {
        Some(Width::Bits64) => Ok(()),
        Some(width) => Err(format!(
            "indirect {instruction} target must be 64-bit, found {}-bit operand",
            width.bits()
        )),
        None => Err(format!(
            "indirect {instruction} target must have a known 64-bit width"
        )),
    }
}

fn resolve_print_part<'a>(
    strings: &'a StringTable,
    literal_indexes: &mut HashMap<String, usize>,
    label_name: &str,
    part: &PrintPart,
) -> Result<&'a StringBinding, String> {
    match part {
        PrintPart::Binding(name) => strings
            .bindings
            .get(&(label_name.to_string(), name.clone()))
            .ok_or_else(|| {
                format!("Cannot print unknown binding {name:?} in label {label_name:?}")
            }),
        PrintPart::Literal(_) => {
            let index = literal_indexes.entry(label_name.to_string()).or_insert(0);
            *index += 1;

            strings
                .literals
                .get(&(label_name.to_string(), *index))
                .ok_or_else(|| String::from("Internal error: missing print literal"))
        }
        PrintPart::Operand(_) | PrintPart::FormattedOperand { .. } => {
            Err(String::from("Internal error: operand print is runtime"))
        }
    }
}

fn emit_data(asm: &mut String, memory: &[MemoryDeclaration], labels: &LabelSymbols) {
    if memory
        .iter()
        .all(|declaration| matches!(declaration, MemoryDeclaration::Buffer { .. }))
    {
        return;
    }

    asm.push_str(".section .data\n");

    for declaration in memory {
        match declaration {
            MemoryDeclaration::Scalar { name, width, value } => {
                asm.push_str(&format!("{name}:\n"));
                asm.push_str(&format!(
                    "  {} {}\n",
                    width.directive(),
                    format_data_scalar(*width, *value)
                ));
            }
            MemoryDeclaration::FloatScalar { name, width, value } => {
                asm.push_str(&format!("{name}:\n"));
                asm.push_str(&format!("  {} {value}\n", width.directive()));
            }
            MemoryDeclaration::Array {
                name,
                width,
                values,
            } => {
                asm.push_str(&format!("{name}:\n"));
                emit_memory_values(asm, *width, values, labels);
            }
            MemoryDeclaration::Repeat {
                name,
                width,
                count,
                value,
            } => {
                asm.push_str(&format!("{name}:\n"));
                for _ in 0..*count {
                    emit_memory_value(asm, *width, value, labels);
                }
            }
            MemoryDeclaration::Buffer { .. } => {}
        }
    }

    asm.push('\n');
}

fn emit_memory_values(
    asm: &mut String,
    width: MemoryWidth,
    values: &[MemoryValue],
    labels: &LabelSymbols,
) {
    for value in values {
        emit_memory_value(asm, width, value, labels);
    }
}

fn emit_memory_value(
    asm: &mut String,
    width: MemoryWidth,
    value: &MemoryValue,
    labels: &LabelSymbols,
) {
    match value {
        MemoryValue::Integer(value) => {
            asm.push_str(&format!(
                "  {} {}\n",
                width.directive(),
                format_data_scalar(width, *value)
            ));
        }
        MemoryValue::Addr { target } => {
            asm.push_str(&format!("  .quad {}\n", labels.emit_label(target)));
        }
    }
}

fn emit_static_data(asm: &mut String, data: &[DataDeclaration], labels: &LabelSymbols) {
    for declaration in data {
        let flags = if declaration.keep { "aR" } else { "a" };
        asm.push_str(&format!(
            ".section {}, \"{}\", @progbits\n",
            declaration.section, flags
        ));

        if declaration.export {
            asm.push_str(&format!(".global {}\n", declaration.name));
        }

        if let Some(align) = declaration.align {
            asm.push_str(&format!(".balign {align}\n"));
        }

        asm.push_str(&format!("{}:\n", declaration.name));

        for item in &declaration.items {
            match item {
                DataItem::Scalar { width, value } => {
                    asm.push_str(&format!(
                        "  {} {}\n",
                        width.directive(),
                        format_data_scalar(*width, *value)
                    ));
                }
                DataItem::Addr { target } => {
                    asm.push_str(&format!("  .quad {}\n", labels.emit_label(target)));
                }
                DataItem::Zero { count } => {
                    asm.push_str(&format!("  .zero {count}\n"));
                }
                DataItem::Label { name } => {
                    asm.push_str(&format!("{name}:\n"));
                }
            }
        }

        asm.push('\n');
    }
}

fn format_data_scalar(width: MemoryWidth, value: i128) -> String {
    if matches!(width, MemoryWidth::U64 | MemoryWidth::Ptr) {
        (value as u64).to_string()
    } else {
        value.to_string()
    }
}

fn emit_bss(asm: &mut String, memory: &[MemoryDeclaration]) {
    let buffers: Vec<_> = memory
        .iter()
        .filter_map(|declaration| match declaration {
            MemoryDeclaration::Scalar { .. }
            | MemoryDeclaration::FloatScalar { .. }
            | MemoryDeclaration::Array { .. }
            | MemoryDeclaration::Repeat { .. } => None,
            MemoryDeclaration::Buffer { name, width, count } => Some((name, width, count)),
        })
        .collect();

    if buffers.is_empty() {
        return;
    }

    asm.push_str(".section .bss\n");

    for (name, width, count) in buffers {
        asm.push_str(&format!("{name}:\n"));
        asm.push_str(&format!("  .zero {}\n", width.size() * count));
    }

    asm.push('\n');
}

fn emit_rodata(asm: &mut String, strings: &[StringBinding], floats: &[FloatBinding]) {
    if strings.is_empty() && floats.is_empty() {
        return;
    }

    let mut bindings: Vec<_> = strings.iter().collect();
    bindings.sort_by(|left, right| left.asm_label.cmp(&right.asm_label));

    asm.push_str(".section .rodata\n");

    for string in bindings {
        asm.push_str(&format!("{}:\n", string.asm_label));

        if string.value.is_empty() {
            asm.push_str("  .byte 0\n");
        } else {
            let bytes = string
                .value
                .as_bytes()
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(", ");

            asm.push_str(&format!("  .byte {bytes}\n"));
        }
    }

    let mut float_bindings: Vec<_> = floats.iter().collect();
    float_bindings.sort_by(|left, right| left.asm_label.cmp(&right.asm_label));

    for float in float_bindings {
        asm.push_str(&format!("{}:\n", float.asm_label));
        asm.push_str(&format!("  {} {}\n", float.width.directive(), float.value));
    }

    asm.push('\n');
}

fn emit_print_string_instruction(asm: &mut String, string: &StringBinding) {
    emit_print_volatile_pushes(asm);
    asm.push_str("  mov rax, 1\n");
    asm.push_str("  mov rdi, 1\n");
    asm.push_str(&format!("  lea rsi, [rip + {}]\n", string.asm_label));
    asm.push_str(&format!("  mov rdx, {}\n", string.value.len()));
    asm.push_str("  syscall\n");
    emit_print_volatile_pops(asm);
}

fn emit_print_volatile_pushes(asm: &mut String) {
    asm.push_str("  push rax\n");
    asm.push_str("  push rcx\n");
    asm.push_str("  push rdi\n");
    asm.push_str("  push rsi\n");
    asm.push_str("  push rdx\n");
    asm.push_str("  push r11\n");
}

fn emit_print_volatile_pops(asm: &mut String) {
    asm.push_str("  pop r11\n");
    asm.push_str("  pop rdx\n");
    asm.push_str("  pop rsi\n");
    asm.push_str("  pop rdi\n");
    asm.push_str("  pop rcx\n");
    asm.push_str("  pop rax\n");
}

fn emit_print_operand_instruction(
    asm: &mut String,
    operand: &Operand,
    format: PrintFormat,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    index: usize,
) -> Result<(), String> {
    let format = resolve_print_format(format, operand, strings, label_name, stack)?;

    if operand_uses_xmm_register(operand) || is_float_memory_operand(operand, strings, stack)? {
        return Err(String::from(
            "print operand does not support floating-point values yet",
        ));
    }

    if matches!(operand, Operand::Pointer(_)) {
        return Err(String::from(
            "print operand cannot be an address-of operand",
        ));
    }

    if operand_uses_high_byte_register(operand) {
        return Err(String::from(
            "print operand cannot use high-byte registers ah, bh, ch, or dh",
        ));
    }

    validate_print_format_operand(format, operand, strings, label_name, stack)?;

    emit_print_volatile_pushes(asm);
    load_print_operand(asm, operand, format, strings, label_name, stack)?;

    let loop_label = format!(".L.__subsea.{label_name}.print_{index}_loop");
    let negative_label = format!(".L.__subsea.{label_name}.print_{index}_negative");
    let digits_label = format!(".L.__subsea.{label_name}.print_{index}_digits");
    let prefix_done_label = format!(".L.__subsea.{label_name}.print_{index}_prefix_done");
    let digit_decimal_label = format!(".L.__subsea.{label_name}.print_{index}_digit_decimal");

    asm.push_str("  push rbx\n");
    asm.push_str("  sub rsp, 80\n");
    asm.push_str("  lea rsi, [rsp + 80]\n");
    match format {
        PrintFormat::Infer => unreachable!(),
        PrintFormat::SignedDecimal(_) => {
            asm.push_str("  mov rbx, 10\n");
            asm.push_str("  mov byte ptr [rsp], 0\n");
            asm.push_str("  cmp rax, 0\n");
            asm.push_str(&format!("  jl {negative_label}\n"));
            asm.push_str(&format!("  jmp {digits_label}\n"));
            asm.push_str(&format!("{negative_label}:\n"));
            asm.push_str("  neg rax\n");
            asm.push_str("  mov byte ptr [rsp], 45\n");
        }
        PrintFormat::UnsignedDecimal(_) => {
            asm.push_str("  mov rbx, 10\n");
        }
        PrintFormat::Hex | PrintFormat::Pointer => {
            asm.push_str("  mov rbx, 16\n");
            asm.push_str("  mov byte ptr [rsp], 48\n");
            asm.push_str("  mov byte ptr [rsp + 1], 120\n");
        }
        PrintFormat::Binary => {
            asm.push_str("  mov rbx, 2\n");
            asm.push_str("  mov byte ptr [rsp], 48\n");
            asm.push_str("  mov byte ptr [rsp + 1], 98\n");
        }
    }
    asm.push_str(&format!("{digits_label}:\n"));
    asm.push_str(&format!("{loop_label}:\n"));
    asm.push_str("  xor rdx, rdx\n");
    asm.push_str("  div rbx\n");
    if matches!(format, PrintFormat::Hex | PrintFormat::Pointer) {
        asm.push_str("  cmp dl, 9\n");
        asm.push_str(&format!("  jbe {digit_decimal_label}\n"));
        asm.push_str("  add dl, 87\n");
        asm.push_str(&format!("  jmp {prefix_done_label}\n"));
        asm.push_str(&format!("{digit_decimal_label}:\n"));
        asm.push_str("  add dl, 48\n");
        asm.push_str(&format!("{prefix_done_label}:\n"));
    } else {
        asm.push_str("  add dl, 48\n");
    }
    asm.push_str("  sub rsi, 1\n");
    asm.push_str("  mov byte ptr [rsi], dl\n");
    asm.push_str("  cmp rax, 0\n");
    asm.push_str(&format!("  jne {loop_label}\n"));
    match format {
        PrintFormat::Infer => unreachable!(),
        PrintFormat::SignedDecimal(_) => {
            asm.push_str("  cmp byte ptr [rsp], 45\n");
            asm.push_str(&format!("  jne {prefix_done_label}\n"));
            asm.push_str("  sub rsi, 1\n");
            asm.push_str("  mov byte ptr [rsi], 45\n");
            asm.push_str(&format!("{prefix_done_label}:\n"));
        }
        PrintFormat::Hex | PrintFormat::Pointer | PrintFormat::Binary => {
            let marker = match format {
                PrintFormat::Infer => unreachable!(),
                PrintFormat::Binary => 98,
                _ => 120,
            };
            asm.push_str("  sub rsi, 1\n");
            asm.push_str(&format!("  mov byte ptr [rsi], {marker}\n"));
            asm.push_str("  sub rsi, 1\n");
            asm.push_str("  mov byte ptr [rsi], 48\n");
        }
        PrintFormat::UnsignedDecimal(_) => {}
    }
    asm.push_str("  lea rdx, [rsp + 80]\n");
    asm.push_str("  sub rdx, rsi\n");
    asm.push_str("  mov rax, 1\n");
    asm.push_str("  mov rdi, 1\n");
    asm.push_str("  syscall\n");
    asm.push_str("  add rsp, 80\n");
    asm.push_str("  pop rbx\n");
    emit_print_volatile_pops(asm);

    Ok(())
}

fn validate_print_format_operand(
    format: PrintFormat,
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(operand, Operand::Immediate(_)) {
        return Ok(());
    }

    if let Some(expected) = print_format_operand_width(format) {
        return match operand_width(operand, strings, label_name, stack)? {
            Some(width) if width == memory_width_bits(expected) => Ok(()),
            Some(width) => Err(format!(
                "{} print operand must be {}-bit, found {}-bit operand",
                print_format_name(format),
                memory_width_bits(expected).bits(),
                width.bits()
            )),
            None => Err(format!(
                "{} print operand must have a known {}-bit width",
                print_format_name(format),
                memory_width_bits(expected).bits()
            )),
        };
    }

    match operand_width(operand, strings, label_name, stack)? {
        Some(Width::Bits64) => Ok(()),
        Some(width) => Err(format!(
            "{} print operand must be 64-bit, found {}-bit operand",
            print_format_name(format),
            width.bits()
        )),
        None => Err(format!(
            "{} print operand must be an integer immediate, const, 64-bit register, or 64-bit memory operand",
            print_format_name(format)
        )),
    }
}

fn print_format_operand_width(format: PrintFormat) -> Option<MemoryWidth> {
    match format {
        PrintFormat::SignedDecimal(width) | PrintFormat::UnsignedDecimal(width) => Some(width),
        PrintFormat::Pointer => Some(MemoryWidth::Ptr),
        PrintFormat::Hex | PrintFormat::Binary | PrintFormat::Infer => None,
    }
}

fn resolve_print_format(
    format: PrintFormat,
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<PrintFormat, String> {
    if format != PrintFormat::Infer {
        return Ok(format);
    }

    match operand {
        Operand::Immediate(_) => Ok(PrintFormat::SignedDecimal(MemoryWidth::I64)),
        Operand::Ident(name) => {
            if let Some((_, width)) = stack_scalar_slot(stack, name) {
                return Ok(infer_print_format_for_width(width));
            }

            if stack_string_slot(stack, name).is_some() {
                return Err(format!(
                    "String stack variable {name:?} uses string formatting; pass it to {{}} by name"
                ));
            }

            if let Some(binding) = strings
                .integers
                .get(&(label_name.to_string(), name.clone()))
            {
                return Ok(binding
                    .width
                    .map(infer_print_format_for_width)
                    .unwrap_or(PrintFormat::SignedDecimal(MemoryWidth::I64)));
            }

            Err(format!(
                "Cannot infer print format for {name:?}; use {{i64}}, {{u64}}, {{x}}, {{b}}, or {{ptr}}"
            ))
        }
        Operand::Dereference { address, width } => {
            let Some(width) = resolve_memory_width(address, *width, strings)? else {
                return Err(String::from(
                    "Cannot infer print format for memory operand without a known width; use {i64}, {u64}, {x}, {b}, or {ptr}",
                ));
            };

            if width.is_float() {
                return Err(String::from(
                    "print operand does not support floating-point values yet",
                ));
            }

            Ok(infer_print_format_for_width(width))
        }
        Operand::StringProperty { property, .. } => match property {
            StringProperty::Len => Ok(PrintFormat::UnsignedDecimal(MemoryWidth::U64)),
            StringProperty::Ptr => Ok(PrintFormat::Pointer),
        },
        Operand::Register(register) => Err(format!(
            "Cannot infer print format for register {register}; use {{i64}}, {{u64}}, {{x}}, {{b}}, or {{ptr}}"
        )),
        _ => Err(String::from(
            "Cannot infer print format for this operand; use {i64}, {u64}, {x}, {b}, or {ptr}",
        )),
    }
}

fn infer_print_format_for_width(width: MemoryWidth) -> PrintFormat {
    match width {
        MemoryWidth::I8 | MemoryWidth::I16 | MemoryWidth::I32 | MemoryWidth::I64 => {
            PrintFormat::SignedDecimal(width)
        }
        MemoryWidth::U8 | MemoryWidth::U16 | MemoryWidth::U32 | MemoryWidth::U64 => {
            PrintFormat::UnsignedDecimal(width)
        }
        MemoryWidth::Ptr => PrintFormat::Pointer,
        MemoryWidth::F32 | MemoryWidth::F64 => PrintFormat::Infer,
    }
}

fn print_format_name(format: PrintFormat) -> &'static str {
    match format {
        PrintFormat::Infer => "inferred",
        PrintFormat::SignedDecimal(MemoryWidth::I8) => "i8",
        PrintFormat::SignedDecimal(MemoryWidth::I16) => "i16",
        PrintFormat::SignedDecimal(MemoryWidth::I32) => "i32",
        PrintFormat::SignedDecimal(MemoryWidth::I64) => "i64",
        PrintFormat::UnsignedDecimal(MemoryWidth::U8) => "u8",
        PrintFormat::UnsignedDecimal(MemoryWidth::U16) => "u16",
        PrintFormat::UnsignedDecimal(MemoryWidth::U32) => "u32",
        PrintFormat::UnsignedDecimal(MemoryWidth::U64) => "u64",
        PrintFormat::SignedDecimal(_) | PrintFormat::UnsignedDecimal(_) => "integer",
        PrintFormat::Hex => "hex",
        PrintFormat::Binary => "binary",
        PrintFormat::Pointer => "pointer",
    }
}

fn emit_print_stack_string_instruction(
    asm: &mut String,
    name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let (ptr_offset, len_offset) = stack_string_slot(stack, name)
        .ok_or_else(|| format!("Unknown string stack variable {name:?}"))?;

    emit_print_volatile_pushes(asm);
    asm.push_str("  mov rax, 1\n");
    asm.push_str("  mov rdi, 1\n");
    asm.push_str(&format!("  mov rsi, qword ptr [rbp - {ptr_offset}]\n"));
    asm.push_str(&format!("  mov rdx, qword ptr [rbp - {len_offset}]\n"));
    asm.push_str("  syscall\n");
    emit_print_volatile_pops(asm);

    Ok(())
}

fn emit_stack_string_initializer(
    asm: &mut String,
    name: &str,
    value: &StringInitializer,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let (ptr_offset, len_offset) = stack_string_slot(stack, name)
        .ok_or_else(|| format!("Unknown string stack variable {name:?}"))?;

    match value {
        StringInitializer::Literal(_) => {
            let string = strings
                .stack_strings
                .get(&(label_name.to_string(), name.to_string()))
                .ok_or_else(|| format!("Unknown string literal for stack variable {name:?}"))?;

            emit_stack_string_address(asm, &string.asm_label, ptr_offset);
            asm.push_str(&format!(
                "  mov qword ptr [rbp - {len_offset}], {}\n",
                string.value.len()
            ));
        }
        StringInitializer::Slice { ptr, len } => {
            emit_stack_string_slice_pointer(asm, ptr, strings, label_name, stack, ptr_offset)?;
            emit_stack_string_slice_len(asm, len, strings, label_name, stack, len_offset)?;
        }
    }

    Ok(())
}

fn emit_stack_string_address(asm: &mut String, label: &str, ptr_offset: usize) {
    asm.push_str("  push r10\n");
    asm.push_str(&format!("  lea r10, [rip + {label}]\n"));
    asm.push_str(&format!("  mov qword ptr [rbp - {ptr_offset}], r10\n"));
    asm.push_str("  pop r10\n");
}

fn emit_stack_string_slice_pointer(
    asm: &mut String,
    ptr: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    ptr_offset: usize,
) -> Result<(), String> {
    match ptr {
        Operand::Pointer(name) => {
            emit_stack_string_address(asm, name, ptr_offset);
            Ok(())
        }
        Operand::AddressOf(address) => {
            asm.push_str("  push r10\n");
            let address = emit_address(address);
            asm.push_str(&format!("  lea r10, [{address}]\n"));
            asm.push_str(&format!("  mov qword ptr [rbp - {ptr_offset}], r10\n"));
            asm.push_str("  pop r10\n");
            Ok(())
        }
        Operand::Register(name) => match register_width(name) {
            Some(Width::Bits64) => {
                asm.push_str(&format!("  mov qword ptr [rbp - {ptr_offset}], {name}\n"));
                Ok(())
            }
            Some(width) => Err(format!(
                "slice pointer must be a 64-bit register or address-of operand, found {}-bit register",
                width.bits()
            )),
            None => Err(String::from(
                "slice pointer must be a 64-bit integer register or address-of operand",
            )),
        },
        operand => {
            let operand = emit_operand(operand, strings, label_name, stack)?;
            Err(format!(
                "slice pointer must be a 64-bit register or address-of operand, found {operand}"
            ))
        }
    }
}

fn emit_stack_string_slice_len(
    asm: &mut String,
    len: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    len_offset: usize,
) -> Result<(), String> {
    if let Some(value) = immediate_value(len, strings, label_name, stack) {
        validate_immediate_range(value, ImmediateDestination::Memory(MemoryWidth::U64))?;
        asm.push_str(&format!("  mov qword ptr [rbp - {len_offset}], {value}\n"));
        return Ok(());
    }

    match operand_width(len, strings, label_name, stack)? {
        Some(Width::Bits64) => {
            let emitted_len = emit_operand(len, strings, label_name, stack)?;
            if is_memory_operand(len, stack) {
                asm.push_str("  push r10\n");
                asm.push_str(&format!("  mov r10, {emitted_len}\n"));
                asm.push_str(&format!("  mov qword ptr [rbp - {len_offset}], r10\n"));
                asm.push_str("  pop r10\n");
            } else {
                asm.push_str(&format!(
                    "  mov qword ptr [rbp - {len_offset}], {emitted_len}\n"
                ));
            }
            Ok(())
        }
        Some(width) => Err(format!(
            "slice length must be 64-bit, found {}-bit operand",
            width.bits()
        )),
        None => Err(String::from(
            "slice length must be an integer immediate, const, 64-bit register, or 64-bit stack variable",
        )),
    }
}

fn emit_read_instruction(
    asm: &mut String,
    src: &ReadSource,
    dst: &Operand,
    len: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    emit_read_len_arg(asm, len, strings, label_name, stack)?;
    emit_read_dst_arg(asm, dst)?;
    emit_read_src_arg(asm, src);
    asm.push_str("  mov rax, 0\n");
    asm.push_str("  syscall\n");

    Ok(())
}

fn emit_linux_reserve_assignment(
    asm: &mut String,
    dst: &Operand,
    len: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    emit_reserve_len_arg(asm, len, strings, label_name, stack)?;
    asm.push_str("  mov rax, 9\n");
    asm.push_str("  mov rdi, 0\n");
    asm.push_str("  mov rdx, 3\n");
    asm.push_str("  mov r10, 34\n");
    asm.push_str("  mov r8, -1\n");
    asm.push_str("  mov r9, 0\n");
    asm.push_str("  syscall\n");

    if dst != &Operand::Register(String::from("rax")) {
        emit_copy_instruction(
            asm,
            &Operand::Register(String::from("rax")),
            dst,
            strings,
            label_name,
            stack,
        )?;
    }

    Ok(())
}

fn emit_release_instruction(
    asm: &mut String,
    ptr: &Operand,
    len: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    emit_release_ptr_arg(asm, ptr, strings, label_name, stack)?;
    emit_release_len_arg(asm, len, strings, label_name, stack)?;
    asm.push_str("  mov rax, 11\n");
    asm.push_str("  syscall\n");

    Ok(())
}

fn emit_reserve_len_arg(
    asm: &mut String,
    len: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    emit_linux_memory_size_arg(asm, "rsi", "reserve size", len, strings, label_name, stack)
}

fn emit_release_ptr_arg(
    asm: &mut String,
    ptr: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    emit_linux_memory_pointer_arg(asm, ptr, strings, label_name, stack)
}

fn emit_release_len_arg(
    asm: &mut String,
    len: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(len, Operand::Register(register) if register == "rdi") {
        return Err(String::from(
            "release size cannot use rdi because release uses rdi for the pointer",
        ));
    }

    emit_linux_memory_size_arg(asm, "rsi", "release size", len, strings, label_name, stack)
}

fn emit_linux_memory_pointer_arg(
    asm: &mut String,
    ptr: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match ptr {
        Operand::Pointer(name) => {
            asm.push_str(&format!("  lea rdi, [rip + {name}]\n"));
            Ok(())
        }
        _ => match operand_width(ptr, strings, label_name, stack)? {
            Some(Width::Bits64) => {
                let ptr = emit_operand(ptr, strings, label_name, stack)?;
                asm.push_str(&format!("  mov rdi, {ptr}\n"));
                Ok(())
            }
            Some(width) => Err(format!(
                "release pointer must be 64-bit, found {}-bit operand",
                width.bits()
            )),
            None => Err(String::from(
                "release pointer must be address-of memory, a 64-bit register, pointer memory, or a 64-bit stack variable",
            )),
        },
    }
}

fn emit_linux_memory_size_arg(
    asm: &mut String,
    dst_register: &str,
    description: &str,
    len: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if let Some(value) = immediate_value(len, strings, label_name, stack) {
        validate_immediate_range(value, ImmediateDestination::Memory(MemoryWidth::U64))?;
        asm.push_str(&format!("  mov {dst_register}, {value}\n"));
        return Ok(());
    }

    match operand_width(len, strings, label_name, stack)? {
        Some(Width::Bits64) => {
            let len = emit_operand(len, strings, label_name, stack)?;
            asm.push_str(&format!("  mov {dst_register}, {len}\n"));
            Ok(())
        }
        Some(width) => Err(format!(
            "{description} must be 64-bit, found {}-bit operand",
            width.bits()
        )),
        None => Err(format!(
            "{description} must be an integer immediate, const, 64-bit register, or 64-bit stack variable"
        )),
    }
}

fn emit_read_src_arg(asm: &mut String, src: &ReadSource) {
    match src {
        ReadSource::Stdin => asm.push_str("  mov rdi, 0\n"),
    }
}

fn emit_read_dst_arg(asm: &mut String, dst: &Operand) -> Result<(), String> {
    match dst {
        Operand::Pointer(name) => {
            asm.push_str(&format!("  lea rsi, [rip + {name}]\n"));
            Ok(())
        }
        Operand::Register(name) => {
            if name == "rdx" {
                return Err(String::from(
                    "read destination cannot use rdx because read uses rdx for the buffer size",
                ));
            }

            match register_width(name) {
                Some(Width::Bits64) => {
                    asm.push_str(&format!("  mov rsi, {name}\n"));
                    Ok(())
                }
                Some(width) => Err(format!(
                    "read destination must be address-of memory or a 64-bit pointer register, found {}-bit register",
                    width.bits()
                )),
                None => Err(String::from(
                    "read destination must be address-of memory or a 64-bit integer register",
                )),
            }
        }
        _ => Err(String::from(
            "read destination must be address-of memory or a 64-bit pointer register",
        )),
    }
}

fn emit_read_len_arg(
    asm: &mut String,
    len: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if let Some(value) = immediate_value(len, strings, label_name, stack) {
        validate_immediate_range(value, ImmediateDestination::Memory(MemoryWidth::U64))?;
        asm.push_str(&format!("  mov rdx, {value}\n"));
        return Ok(());
    }

    match operand_width(len, strings, label_name, stack)? {
        Some(Width::Bits64) => {
            let len = emit_operand(len, strings, label_name, stack)?;
            asm.push_str(&format!("  mov rdx, {len}\n"));
            Ok(())
        }
        Some(width) => Err(format!(
            "read buffer size must be 64-bit, found {}-bit operand",
            width.bits()
        )),
        None => Err(String::from(
            "read buffer size must be an integer immediate, const, 64-bit register, or 64-bit stack variable",
        )),
    }
}

fn load_print_operand(
    asm: &mut String,
    operand: &Operand,
    format: PrintFormat,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(operand, Operand::Immediate(_)) {
        let operand = emit_operand(operand, strings, label_name, stack)?;
        asm.push_str(&format!("  mov rax, {operand}\n"));
        return Ok(());
    }

    let operand = emit_operand(operand, strings, label_name, stack)?;

    match format {
        PrintFormat::SignedDecimal(MemoryWidth::I8) => {
            asm.push_str(&format!("  movsx rax, {operand}\n"));
        }
        PrintFormat::SignedDecimal(MemoryWidth::I16) => {
            asm.push_str(&format!("  movsx rax, {operand}\n"));
        }
        PrintFormat::SignedDecimal(MemoryWidth::I32) => {
            asm.push_str(&format!("  movsxd rax, {operand}\n"));
        }
        PrintFormat::SignedDecimal(MemoryWidth::I64) => {
            asm.push_str(&format!("  mov rax, {operand}\n"));
        }
        PrintFormat::UnsignedDecimal(MemoryWidth::U8) => {
            asm.push_str(&format!("  movzx rax, {operand}\n"));
        }
        PrintFormat::UnsignedDecimal(MemoryWidth::U16) => {
            asm.push_str(&format!("  movzx rax, {operand}\n"));
        }
        PrintFormat::UnsignedDecimal(MemoryWidth::U32) => {
            asm.push_str(&format!("  mov eax, {operand}\n"));
        }
        PrintFormat::UnsignedDecimal(MemoryWidth::U64)
        | PrintFormat::Hex
        | PrintFormat::Binary
        | PrintFormat::Pointer => {
            asm.push_str(&format!("  mov rax, {operand}\n"));
        }
        PrintFormat::Infer | PrintFormat::SignedDecimal(_) | PrintFormat::UnsignedDecimal(_) => {
            unreachable!()
        }
    }

    Ok(())
}

fn emit_binary_instruction(
    asm: &mut String,
    opcode: &str,
    src: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_binary_operands(opcode, src, dst, strings, label_name, stack)?;

    let src = emit_operand(src, strings, label_name, stack)?;
    let dst = emit_operand(dst, strings, label_name, stack)?;
    asm.push_str(&format!("  {opcode} {dst}, {src}\n"));

    Ok(())
}

fn emit_integer_math_instruction(
    asm: &mut String,
    op: MathOp,
    src: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if is_shift_math_op(op) {
        return emit_shift_instruction(
            asm,
            integer_math_opcode(op),
            src,
            dst,
            strings,
            label_name,
            stack,
        );
    }

    emit_binary_instruction(
        asm,
        integer_math_opcode(op),
        src,
        dst,
        strings,
        label_name,
        stack,
    )
}

fn emit_shift_instruction(
    asm: &mut String,
    opcode: &str,
    count: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_shift_operands(opcode, count, dst, strings, label_name, stack)?;

    let count = emit_operand(count, strings, label_name, stack)?;
    let dst = emit_operand(dst, strings, label_name, stack)?;
    asm.push_str(&format!("  {opcode} {dst}, {count}\n"));

    Ok(())
}

fn emit_bitwise_unary_instruction(
    asm: &mut String,
    op: BitwiseUnaryOp,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let opcode = match op {
        BitwiseUnaryOp::Not => "not",
    };

    validate_bitwise_unary_operand(opcode, dst, strings, stack)?;
    let dst = emit_operand(dst, strings, label_name, stack)?;
    asm.push_str(&format!("  {opcode} {dst}\n"));

    Ok(())
}

fn emit_assignment(
    asm: &mut String,
    dst: &AssignmentTarget,
    value: &AssignmentValue,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match value {
        AssignmentValue::Operand(src) => {
            let dst = assignment_operand_target(dst)?;
            if matches!(dst, Operand::Dereference { .. })
                && let Some(value) = string_bytes_assignment_source(src, strings, label_name)?
            {
                return emit_string_bytes_assignment(asm, dst, value);
            }

            emit_copy_instruction(asm, src, dst, strings, label_name, stack)
        }
        AssignmentValue::BitwiseUnary { op, operand } => {
            let dst = assignment_operand_target(dst)?;
            emit_copy_instruction(asm, operand, dst, strings, label_name, stack)?;
            emit_bitwise_unary_instruction(asm, *op, dst, strings, label_name, stack)
        }
        AssignmentValue::Condition(condition) => {
            let dst = assignment_operand_target(dst)?;
            emit_boolean_condition_assignment(asm, dst, condition, strings, label_name, stack)
        }
        AssignmentValue::Expression(expression) => {
            let dst = assignment_operand_target(dst)?;
            emit_expression_assignment(asm, dst, expression, strings, label_name, stack)
        }
        AssignmentValue::Binary { op, lhs, rhs } => {
            let dst = assignment_operand_target(dst)?;

            if *op == MathOp::Power {
                return emit_power_assignment(asm, dst, lhs, rhs, strings, label_name, stack);
            }

            if integer_op_can_be_float(*op)
                && let Some(width) =
                    resolve_float_binary_width(lhs, rhs, strings, label_name, stack)?
            {
                return emit_float_binary_operand_assignment(
                    asm,
                    dst,
                    width,
                    float_math_op_from_integer_op(*op),
                    lhs,
                    rhs,
                    strings,
                    label_name,
                    stack,
                );
            }

            if is_ambiguous_float_binary_operand(lhs)
                || is_ambiguous_float_binary_operand(rhs)
                || is_ambiguous_float_binary_operand(dst)
            {
                return Err(format!(
                    "Floating-point arithmetic width is ambiguous; use f32{} or f64{}",
                    math_op_symbol(*op),
                    math_op_symbol(*op)
                ));
            }

            if !matches!(dst, Operand::Register(_)) && *op == MathOp::Multiply {
                return Err(String::from(
                    "Multiply assignment destination must be a register for now",
                ));
            }

            if lhs == dst {
                return emit_integer_math_instruction(
                    asm, *op, rhs, dst, strings, label_name, stack,
                );
            }

            if rhs == dst {
                match op {
                    op if is_commutative_math_op(*op) => {
                        return emit_integer_math_instruction(
                            asm, *op, lhs, dst, strings, label_name, stack,
                        );
                    }
                    MathOp::Subtract => {
                        let dst_operand = emit_operand(dst, strings, label_name, stack)?;
                        asm.push_str(&format!("  neg {dst_operand}\n"));

                        return emit_binary_instruction(
                            asm, "add", lhs, dst, strings, label_name, stack,
                        );
                    }
                    op => {
                        return Err(format!(
                            "Binary assignment destination cannot also be the right operand for {}",
                            math_op_symbol(*op)
                        ));
                    }
                }
            }

            validate_binary_assignment_does_not_clobber_rhs_address(dst, rhs)?;
            if is_shift_math_op(*op) {
                validate_shift_assignment_does_not_clobber_count(dst, rhs)?;
            }

            {
                emit_copy_instruction(asm, lhs, dst, strings, label_name, stack)?;
                emit_integer_math_instruction(asm, *op, rhs, dst, strings, label_name, stack)
            }
        }
        AssignmentValue::FloatBinary {
            width,
            op,
            lhs,
            rhs,
        } => emit_float_binary_assignment(
            asm, dst, *width, *op, lhs, rhs, strings, label_name, stack,
        ),
        AssignmentValue::IntrinsicCall { op, width, args } => {
            emit_intrinsic_call_assignment(asm, dst, *op, *width, args, strings, label_name, stack)
        }
        AssignmentValue::LinuxReserve { len } => {
            let dst = assignment_operand_target(dst)?;
            emit_linux_reserve_assignment(asm, dst, len, strings, label_name, stack)
        }
        AssignmentValue::StringBytes { value } => {
            let dst = assignment_operand_target(dst)?;
            emit_string_bytes_assignment(asm, dst, value)
        }
        AssignmentValue::WideMultiply { signed, lhs, rhs } => emit_wide_math_assignment(
            asm, dst, *signed, false, lhs, rhs, strings, label_name, stack,
        ),
        AssignmentValue::WideDivide { signed, lhs, rhs } => emit_wide_math_assignment(
            asm, dst, *signed, true, lhs, rhs, strings, label_name, stack,
        ),
        AssignmentValue::PairBinary { op, lhs, rhs } => {
            emit_pair_binary_assignment(asm, dst, *op, lhs, rhs)
        }
    }
}

fn assignment_value_uses_linux_reserve(value: &AssignmentValue) -> bool {
    matches!(value, AssignmentValue::LinuxReserve { .. })
}

fn string_bytes_assignment_source<'a>(
    src: &Operand,
    strings: &'a StringTable,
    label_name: &str,
) -> Result<Option<&'a str>, String> {
    let Operand::Ident(name) = src else {
        return Ok(None);
    };

    let key = (label_name.to_string(), name.to_string());
    if strings.integers.contains_key(&key) || strings.float_bindings.contains_key(&key) {
        return Ok(None);
    }

    let Some(binding) = strings.bindings.get(&key) else {
        return Ok(None);
    };

    if binding.value.is_empty() {
        return Err(String::from("String byte assignment cannot be empty"));
    }

    Ok(Some(&binding.value))
}

fn emit_string_bytes_assignment(
    asm: &mut String,
    dst: &Operand,
    value: &str,
) -> Result<(), String> {
    let Operand::Dereference { address, width } = dst else {
        return Err(String::from(
            "String byte assignment destination must be a memory operand",
        ));
    };

    if width.is_some() {
        return Err(String::from(
            "String byte assignment destination cannot specify a memory width",
        ));
    }

    let base = emit_address(address);
    for (index, byte) in value.bytes().enumerate() {
        let address = if index == 0 {
            base.clone()
        } else {
            format!("{base} + {index}")
        };
        asm.push_str(&format!("  mov byte ptr [{address}], {byte}\n"));
    }

    Ok(())
}

fn emit_expression_assignment(
    asm: &mut String,
    dst: &Operand,
    expression: &Expression,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_integer_expression(expression, strings, label_name, stack)?;

    let target = match dst {
        Operand::Register(register) if !is_xmm_register(register) => dst.clone(),
        _ => Operand::Register(expression_temp_register(dst, expression)?),
    };

    emit_expression_to_register(asm, &target, expression, strings, label_name, stack)?;

    if &target != dst {
        emit_copy_instruction(asm, &target, dst, strings, label_name, stack)?;
    }

    Ok(())
}

fn emit_expression_to_register(
    asm: &mut String,
    dst: &Operand,
    expression: &Expression,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let Operand::Register(dst_register) = dst else {
        return Err(String::from(
            "Expression destination must be an integer register",
        ));
    };

    if is_xmm_register(dst_register) {
        return Err(String::from(
            "Expression destination must be an integer register",
        ));
    }

    match expression {
        Expression::Operand(operand) => {
            emit_copy_instruction(asm, operand, dst, strings, label_name, stack)
        }
        Expression::Binary { op, lhs, rhs } => {
            if matches!(op, ExprOp::Math(MathOp::Power) | ExprOp::Power) {
                let Expression::Operand(base) = lhs.as_ref() else {
                    return Err(String::from("Power base must be an operand"));
                };
                let Expression::Operand(exponent) = rhs.as_ref() else {
                    return Err(String::from("Power exponent must be an operand"));
                };

                return emit_power_operation(asm, dst, base, exponent, strings, label_name, stack);
            }

            let precomputed_rhs = if !matches!(op, ExprOp::Math(MathOp::Power) | ExprOp::Power)
                && expression_uses_register_family(rhs, dst_register)
            {
                let temp = Operand::Register(expression_temp_register(dst, expression)?);
                emit_expression_to_register(asm, &temp, rhs, strings, label_name, stack)?;
                Some(temp)
            } else {
                None
            };

            emit_expression_to_register(asm, dst, lhs, strings, label_name, stack)?;

            match op {
                ExprOp::Math(MathOp::Power) | ExprOp::Power => unreachable!(),
                ExprOp::Math(op) => {
                    let rhs_operand = if let Some(rhs_operand) = precomputed_rhs {
                        rhs_operand
                    } else {
                        expression_rhs_operand(asm, dst, rhs, strings, label_name, stack)?
                    };
                    emit_integer_math_instruction(
                        asm,
                        *op,
                        &rhs_operand,
                        dst,
                        strings,
                        label_name,
                        stack,
                    )
                }
                ExprOp::Divide { signed } => {
                    let rhs_operand = if let Some(rhs_operand) = precomputed_rhs {
                        rhs_operand
                    } else {
                        expression_rhs_operand(asm, dst, rhs, strings, label_name, stack)?
                    };
                    emit_division_from_accumulator(
                        asm,
                        *signed,
                        false,
                        dst,
                        &rhs_operand,
                        dst,
                        strings,
                        label_name,
                        stack,
                    )
                }
                ExprOp::Modulo { signed } => {
                    let rhs_operand = if let Some(rhs_operand) = precomputed_rhs {
                        rhs_operand
                    } else {
                        expression_rhs_operand(asm, dst, rhs, strings, label_name, stack)?
                    };
                    emit_division_from_accumulator(
                        asm,
                        *signed,
                        true,
                        dst,
                        &rhs_operand,
                        dst,
                        strings,
                        label_name,
                        stack,
                    )
                }
            }
        }
    }
}

fn expression_rhs_operand(
    asm: &mut String,
    dst: &Operand,
    rhs: &Expression,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<Operand, String> {
    match rhs {
        Expression::Operand(operand) => Ok(operand.clone()),
        Expression::Binary { .. } => {
            let temp = Operand::Register(expression_temp_register(dst, rhs)?);
            emit_expression_to_register(asm, &temp, rhs, strings, label_name, stack)?;
            Ok(temp)
        }
    }
}

fn emit_power_assignment(
    asm: &mut String,
    dst: &Operand,
    lhs: &Operand,
    rhs: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match dst {
        Operand::Register(register) if !is_xmm_register(register) => {
            emit_power_operation(asm, dst, lhs, rhs, strings, label_name, stack)
        }
        _ => Err(String::from(
            "Power destination must be a 64-bit integer register",
        )),
    }
}

fn emit_power_operation(
    asm: &mut String,
    dst: &Operand,
    base: &Operand,
    exponent: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let Operand::Register(dst_register) = dst else {
        return Err(String::from("Power destination must be a register for now"));
    };

    if is_xmm_register(dst_register) {
        return Err(String::from(
            "Power destination must be an integer register",
        ));
    }

    if register_width(dst_register) != Some(Width::Bits64) {
        return Err(String::from(
            "Power destination must be a 64-bit integer register",
        ));
    }

    validate_integer_power_operands(base, exponent, dst, strings, label_name, stack)?;

    if operand_uses_register_family(dst, "r10") || operand_uses_register_family(dst, "r11") {
        return Err(String::from(
            "Power destination cannot use r10 or r11 because they are scratch registers",
        ));
    }

    let exponent_uses_r10 = operand_uses_register_family(exponent, "r10");
    let base_uses_r11_address = operand_address_uses_register_family(base, "r11");

    match (exponent_uses_r10, base_uses_r11_address) {
        (true, true) => {
            return Err(String::from(
                "Power cannot use r10 in the exponent and r11 in the base address because both are scratch registers",
            ));
        }
        (true, false) => {
            emit_power_exponent_load(asm, exponent, strings, label_name, stack)?;
            emit_copy_instruction(
                asm,
                base,
                &Operand::Register(String::from("r10")),
                strings,
                label_name,
                stack,
            )?;
        }
        _ => {
            emit_copy_instruction(
                asm,
                base,
                &Operand::Register(String::from("r10")),
                strings,
                label_name,
                stack,
            )?;
            emit_power_exponent_load(asm, exponent, strings, label_name, stack)?;
        }
    }

    let loop_label = format!(".L.__subsea.{label_name}.pow_{}_loop", asm.len());
    let skip_multiply_label = format!(".L.__subsea.{label_name}.pow_{}_skip_mul", asm.len());
    let done_label = format!(".L.__subsea.{label_name}.pow_{}_done", asm.len());

    asm.push_str(&format!("  mov {dst_register}, 1\n"));
    asm.push_str(&format!("{loop_label}:\n"));
    asm.push_str("  test r11, r11\n");
    asm.push_str(&format!("  je {done_label}\n"));
    asm.push_str("  test r11, 1\n");
    asm.push_str(&format!("  je {skip_multiply_label}\n"));
    asm.push_str(&format!("  imul {dst_register}, r10\n"));
    asm.push_str(&format!("{skip_multiply_label}:\n"));
    asm.push_str("  imul r10, r10\n");
    asm.push_str("  shr r11, 1\n");
    asm.push_str(&format!("  jmp {loop_label}\n"));
    asm.push_str(&format!("{done_label}:\n"));

    Ok(())
}

fn emit_power_exponent_load(
    asm: &mut String,
    exponent: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if let Some(value) = immediate_value(exponent, strings, label_name, stack) {
        if value < 0 {
            return Err(String::from("Power exponent must be non-negative"));
        }
        validate_immediate_range(value, ImmediateDestination::Register(Width::Bits64))?;
        asm.push_str(&format!("  mov r11, {value}\n"));
        return Ok(());
    }

    if operand_uses_high_byte_register(exponent) {
        return Err(String::from(
            "Power exponent cannot use high-byte registers ah, bh, ch, or dh",
        ));
    }

    let width = operand_width(exponent, strings, label_name, stack)?
        .ok_or_else(|| String::from("Power exponent must be an integer operand"))?;
    let exponent = emit_operand(exponent, strings, label_name, stack)?;

    match width {
        Width::Bits64 => asm.push_str(&format!("  mov r11, {exponent}\n")),
        Width::Bits32 => asm.push_str(&format!("  mov r11d, {exponent}\n")),
        Width::Bits16 | Width::Bits8 => asm.push_str(&format!("  movzx r11, {exponent}\n")),
    }

    Ok(())
}

fn emit_division_from_accumulator(
    asm: &mut String,
    signed: bool,
    remainder: bool,
    lhs: &Operand,
    rhs: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_division_operands(lhs, rhs, dst, strings, label_name, stack)?;

    let divisor = materialize_divisor_if_needed(asm, lhs, rhs, strings, label_name, stack)?;
    let rax = Operand::Register(String::from("rax"));
    emit_copy_instruction(asm, lhs, &rax, strings, label_name, stack)?;

    if signed {
        asm.push_str("  cqo\n");
    } else {
        asm.push_str("  xor rdx, rdx\n");
    }

    let opcode = if signed { "idiv" } else { "div" };
    let divisor = emit_operand(&divisor, strings, label_name, stack)?;
    asm.push_str(&format!("  {opcode} {divisor}\n"));

    let result = Operand::Register(if remainder {
        String::from("rdx")
    } else {
        String::from("rax")
    });
    if &result != dst {
        emit_copy_instruction(asm, &result, dst, strings, label_name, stack)?;
    }

    Ok(())
}

fn materialize_divisor_if_needed(
    asm: &mut String,
    lhs: &Operand,
    rhs: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<Operand, String> {
    let needs_temp = is_immediate_operand(rhs, strings, label_name, stack)
        || operand_uses_register_family(rhs, "rax")
        || operand_uses_register_family(rhs, "rdx");

    if !needs_temp {
        return Ok(rhs.clone());
    }

    let temp = division_temp_register(lhs, rhs)?;
    let temp_operand = Operand::Register(temp);
    emit_copy_instruction(asm, rhs, &temp_operand, strings, label_name, stack)?;
    Ok(temp_operand)
}

fn validate_integer_expression(
    expression: &Expression,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match expression {
        Expression::Operand(operand) => {
            if operand_uses_xmm_register(operand)
                || is_float_memory_operand(operand, strings, stack)?
            {
                return Err(String::from(
                    "Arithmetic expressions do not support floating-point operands yet",
                ));
            }
            if matches!(operand, Operand::FloatLiteral(_)) {
                return Err(String::from(
                    "Arithmetic expressions do not support floating-point operands yet",
                ));
            }
            if matches!(operand, Operand::Pointer(_) | Operand::AddressOf(_)) {
                return Err(String::from(
                    "Arithmetic expressions cannot use address-of operands",
                ));
            }
            Ok(())
        }
        Expression::Binary { lhs, rhs, .. } => {
            validate_integer_expression(lhs, strings, label_name, stack)?;
            validate_integer_expression(rhs, strings, label_name, stack)
        }
    }
}

fn validate_integer_power_operands(
    lhs: &Operand,
    rhs: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_wide_math_operand("Power base", lhs, strings, label_name, stack)?;
    validate_power_exponent(rhs, strings, label_name, stack)?;

    if let Some(width) = operand_width(dst, strings, label_name, stack)?
        && width != Width::Bits64
    {
        return Err(format!(
            "Power destination must be 64-bit, found {}-bit operand",
            width.bits()
        ));
    }

    Ok(())
}

fn validate_power_exponent(
    exponent: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(exponent, Operand::Pointer(_) | Operand::AddressOf(_)) {
        return Err(String::from(
            "Power exponent cannot be an address-of operand",
        ));
    }

    if operand_uses_xmm_register(exponent) || is_float_memory_operand(exponent, strings, stack)? {
        return Err(String::from("Power exponent must be an integer operand"));
    }

    if let Some(value) = immediate_value(exponent, strings, label_name, stack)
        && value < 0
    {
        return Err(String::from("Power exponent must be non-negative"));
    }

    Ok(())
}

fn validate_division_operands(
    lhs: &Operand,
    rhs: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_wide_math_operand("Division left operand", lhs, strings, label_name, stack)?;
    validate_wide_math_operand("Division right operand", rhs, strings, label_name, stack)?;

    if let Some(width) = operand_width(dst, strings, label_name, stack)?
        && width != Width::Bits64
    {
        return Err(format!(
            "Division destination must be 64-bit, found {}-bit operand",
            width.bits()
        ));
    }

    Ok(())
}

fn expression_temp_register(dst: &Operand, expression: &Expression) -> Result<String, String> {
    for register in ["r10", "r11", "r8", "r9"] {
        if !operand_uses_register_family(dst, register)
            && !expression_uses_register_family(expression, register)
        {
            return Ok(String::from(register));
        }
    }

    Err(String::from(
        "Arithmetic expression has no available temporary register",
    ))
}

fn division_temp_register(lhs: &Operand, rhs: &Operand) -> Result<String, String> {
    for register in ["r10", "r11", "r8", "r9"] {
        if !operand_uses_register_family(lhs, register)
            && !operand_address_uses_register_family(rhs, register)
        {
            return Ok(String::from(register));
        }
    }

    Err(String::from("Division has no available temporary register"))
}

fn expression_uses_register_family(expression: &Expression, register: &str) -> bool {
    match expression {
        Expression::Operand(operand) => operand_uses_register_family(operand, register),
        Expression::Binary { lhs, rhs, .. } => {
            expression_uses_register_family(lhs, register)
                || expression_uses_register_family(rhs, register)
        }
    }
}

fn emit_boolean_condition_assignment(
    asm: &mut String,
    dst: &Operand,
    condition: &ConditionExpr,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let set_opcode = emit_condition_for_setcc(asm, condition, strings, label_name, stack)?;
    emit_setcc_result(asm, set_opcode, dst, strings, label_name, stack)
}

fn emit_condition_for_setcc(
    asm: &mut String,
    condition: &ConditionExpr,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<&'static str, String> {
    match condition {
        ConditionExpr::Compare(condition) => {
            if resolve_float_compare_width(condition, strings, label_name, stack)?.is_some() {
                return Err(String::from(
                    "Boolean assignment does not support floating-point comparisons yet",
                ));
            }

            let (lhs, rhs, op) = normalize_compare(
                &condition.lhs,
                &condition.rhs,
                condition.op,
                strings,
                label_name,
                stack,
            )?;
            validate_resolved_integer_compare_op(op)?;
            validate_compare_operands(lhs, rhs, strings, label_name, stack)?;

            let use_test = matches!(op, CompareOp::Equal | CompareOp::NotEqual)
                && matches!(lhs, Operand::Register(register) if !is_xmm_register(register))
                && immediate_value(rhs, strings, label_name, stack) == Some(0);
            let lhs = emit_operand(lhs, strings, label_name, stack)?;
            let rhs = emit_operand(rhs, strings, label_name, stack)?;
            if use_test {
                asm.push_str(&format!("  test {lhs}, {lhs}\n"));
            } else {
                asm.push_str(&format!("  cmp {lhs}, {rhs}\n"));
            }

            Ok(compare_set_opcode(op))
        }
        ConditionExpr::BitwiseAndZero { lhs, rhs, op } => {
            validate_test_condition_operands(lhs, rhs, *op, strings, label_name, stack)?;

            let lhs = emit_operand(lhs, strings, label_name, stack)?;
            let rhs = emit_operand(rhs, strings, label_name, stack)?;
            asm.push_str(&format!("  test {lhs}, {rhs}\n"));

            Ok(match op {
                CompareOp::Equal => "sete",
                CompareOp::NotEqual => "setne",
                _ => unreachable!(),
            })
        }
    }
}

fn emit_setcc_result(
    asm: &mut String,
    set_opcode: &str,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_boolean_assignment_destination(dst, strings, stack)?;

    if let Operand::Register(register) = dst
        && register_width(register) == Some(Width::Bits8)
    {
        asm.push_str(&format!("  {set_opcode} {register}\n"));
        return Ok(());
    }

    let temp = boolean_temp_register(dst)?;
    asm.push_str(&format!("  {set_opcode} {}b\n", temp));

    match destination_width(dst, strings, stack)? {
        Some(ImmediateDestination::Register(width)) => {
            let dst = emit_operand(dst, strings, label_name, stack)?;
            asm.push_str(&format!("  movzx {dst}, {}b\n", temp));
            validate_boolean_movzx_width(width)?;
        }
        Some(ImmediateDestination::Memory(width)) => {
            let dst = emit_operand(dst, strings, label_name, stack)?;
            let temp_src = temp_register_for_memory_width(temp, width)?;
            if memory_width_bits(width) != Width::Bits8 {
                asm.push_str(&format!("  movzx {temp}, {}b\n", temp));
            }
            asm.push_str(&format!("  mov {dst}, {temp_src}\n"));
        }
        None => unreachable!(),
    }

    Ok(())
}

fn compare_set_opcode(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Equal => "sete",
        CompareOp::NotEqual => "setne",
        CompareOp::SignedLess => "setl",
        CompareOp::SignedLessEqual => "setle",
        CompareOp::SignedGreater => "setg",
        CompareOp::SignedGreaterEqual => "setge",
        CompareOp::UnsignedLess => "setb",
        CompareOp::UnsignedLessEqual => "setbe",
        CompareOp::UnsignedGreater => "seta",
        CompareOp::UnsignedGreaterEqual => "setae",
        CompareOp::Less | CompareOp::LessEqual | CompareOp::Greater | CompareOp::GreaterEqual => {
            unreachable!()
        }
        CompareOp::FloatEqual(_)
        | CompareOp::FloatNotEqual(_)
        | CompareOp::FloatLess(_)
        | CompareOp::FloatLessEqual(_)
        | CompareOp::FloatGreater(_)
        | CompareOp::FloatGreaterEqual(_) => unreachable!(),
    }
}

fn emit_wide_math_assignment(
    asm: &mut String,
    dst: &AssignmentTarget,
    signed: bool,
    division: bool,
    lhs: &Operand,
    rhs: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let prefix = if division {
        "Widened division"
    } else {
        "Widened multiply"
    };

    validate_wide_math_target(prefix, dst)?;
    validate_wide_math_operand(
        &format!("{prefix} left operand"),
        lhs,
        strings,
        label_name,
        stack,
    )?;
    validate_wide_math_operand(
        &format!("{prefix} right operand"),
        rhs,
        strings,
        label_name,
        stack,
    )?;

    let rhs = materialize_divisor_if_needed(asm, lhs, rhs, strings, label_name, stack)?;

    let rax = Operand::Register(String::from("rax"));
    emit_copy_instruction(asm, lhs, &rax, strings, label_name, stack)?;

    if division {
        if signed {
            asm.push_str("  cqo\n");
        } else {
            asm.push_str("  xor rdx, rdx\n");
        }
    }

    let opcode = match (division, signed) {
        (false, true) => "imul",
        (false, false) => "mul",
        (true, true) => "idiv",
        (true, false) => "div",
    };
    let rhs = emit_operand(&rhs, strings, label_name, stack)?;
    asm.push_str(&format!("  {opcode} {rhs}\n"));

    Ok(())
}

fn emit_pair_binary_assignment(
    asm: &mut String,
    dst: &AssignmentTarget,
    op: PairBinaryOp,
    lhs: &RegisterPair,
    rhs: &RegisterPair,
) -> Result<(), String> {
    let AssignmentTarget::RegisterPair(dst) = dst else {
        return Err(String::from(
            "Pair arithmetic destination must be a register pair",
        ));
    };

    validate_pair_binary_assignment(dst, lhs, rhs)?;

    let (low_opcode, high_opcode) = match op {
        PairBinaryOp::Add => ("add", "adc"),
        PairBinaryOp::Subtract => ("sub", "sbb"),
    };

    asm.push_str(&format!("  {low_opcode} {}, {}\n", dst.low, rhs.low));
    asm.push_str(&format!("  {high_opcode} {}, {}\n", dst.high, rhs.high));

    Ok(())
}

fn validate_pair_binary_assignment(
    dst: &RegisterPair,
    lhs: &RegisterPair,
    rhs: &RegisterPair,
) -> Result<(), String> {
    if dst != lhs {
        return Err(format!(
            "Pair arithmetic left operand must match destination; found {}:{} = {}:{} ...",
            dst.high, dst.low, lhs.high, lhs.low
        ));
    }

    if same_register_family(&dst.high, &dst.low) {
        return Err(format!(
            "Pair arithmetic destination registers must be different, found {}:{}",
            dst.high, dst.low
        ));
    }

    if same_register_family(&rhs.high, &dst.low) {
        return Err(format!(
            "Pair arithmetic right high register {} cannot overlap destination low register {}",
            rhs.high, dst.low
        ));
    }

    validate_pair_binary_register("Pair arithmetic destination high register", &dst.high)?;
    validate_pair_binary_register("Pair arithmetic destination low register", &dst.low)?;
    validate_pair_binary_register("Pair arithmetic right high register", &rhs.high)?;
    validate_pair_binary_register("Pair arithmetic right low register", &rhs.low)?;

    Ok(())
}

fn validate_pair_binary_register(name: &str, register: &str) -> Result<(), String> {
    if is_xmm_register(register) {
        return Err(format!("{name} must be a 64-bit integer register"));
    }

    match register_width(register) {
        Some(Width::Bits64) => Ok(()),
        Some(width) => Err(format!(
            "{name} must be 64-bit, found {}-bit register {register}",
            width.bits()
        )),
        None => Err(format!("{name} must be a register, found {register}")),
    }
}

fn emit_intrinsic_call_assignment(
    asm: &mut String,
    dst: &AssignmentTarget,
    op: IntrinsicOp,
    width: MemoryWidth,
    args: &[Operand],
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let dst = assignment_operand_target(dst)?;

    match (op, width) {
        (
            IntrinsicOp::Ceil | IntrinsicOp::Floor | IntrinsicOp::Round | IntrinsicOp::Trunc,
            MemoryWidth::F32 | MemoryWidth::F64,
        ) => {
            emit_float_rounding_intrinsic(asm, dst, op, width, &args[0], strings, label_name, stack)
        }
        (IntrinsicOp::Ceil | IntrinsicOp::Floor | IntrinsicOp::Round | IntrinsicOp::Trunc, _) => {
            Err(format!(
                "{} only supports f32 or f64; integer rounding is not implemented",
                intrinsic_op_name(op)
            ))
        }
        (IntrinsicOp::Sqrt, MemoryWidth::F32 | MemoryWidth::F64) => {
            emit_float_sqrt_intrinsic(asm, dst, width, &args[0], strings, label_name, stack)
        }
        (
            IntrinsicOp::Sqrt,
            MemoryWidth::I8
            | MemoryWidth::I16
            | MemoryWidth::I32
            | MemoryWidth::I64
            | MemoryWidth::U8
            | MemoryWidth::U16
            | MemoryWidth::U32
            | MemoryWidth::U64,
        ) => emit_integer_sqrt_intrinsic(asm, dst, width, &args[0], strings, label_name, stack),
        (IntrinsicOp::Sqrt, _) => Err(String::from(
            "sqrt integer operands must use a signed or unsigned integer width",
        )),
        (IntrinsicOp::Min | IntrinsicOp::Max, MemoryWidth::F32 | MemoryWidth::F64) => {
            emit_float_min_max_intrinsic(
                asm, dst, op, width, &args[0], &args[1], strings, label_name, stack,
            )
        }
        (IntrinsicOp::Min | IntrinsicOp::Max, MemoryWidth::Ptr) => Err(String::from(
            "min and max do not support ptr width; use an integer width",
        )),
        (IntrinsicOp::Min | IntrinsicOp::Max, _) => emit_integer_min_max_intrinsic(
            asm, dst, op, width, &args[0], &args[1], strings, label_name, stack,
        ),
    }
}

fn emit_float_sqrt_intrinsic(
    asm: &mut String,
    dst: &Operand,
    width: MemoryWidth,
    src: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let memory_destination =
        validate_float_intrinsic_destination("sqrt", dst, width, strings, stack)?;
    validate_float_math_operand("sqrt operand", src, width, strings, label_name, stack)?;

    let dst_register = if memory_destination {
        "xmm15"
    } else {
        let Operand::Register(dst_register) = dst else {
            unreachable!()
        };
        dst_register.as_str()
    };
    let src = emit_float_operand(src, width, strings, label_name, stack)?;
    asm.push_str(&format!(
        "  {} {dst_register}, {src}\n",
        float_sqrt_opcode(width)
    ));

    if memory_destination {
        emit_float_copy_instruction(
            asm,
            &Operand::Register(String::from("xmm15")),
            dst,
            width,
            strings,
            label_name,
            stack,
        )?;
    }

    Ok(())
}

fn emit_integer_sqrt_intrinsic(
    asm: &mut String,
    dst: &Operand,
    width: MemoryWidth,
    src: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let expected = memory_width_bits(width);
    let (dst_register, memory_destination) = match dst {
        Operand::Register(dst_register) if !is_xmm_register(dst_register) => {
            let dst_width = register_width(dst_register).ok_or_else(|| {
                String::from("Integer sqrt intrinsic destination must be an integer register")
            })?;
            if dst_width != expected {
                return Err(format!(
                    "Integer sqrt intrinsic destination must be {}-bit, found {}-bit register",
                    expected.bits(),
                    dst_width.bits()
                ));
            }
            if matches!(dst_register.as_str(), "ah" | "bh" | "ch" | "dh") {
                return Err(String::from(
                    "Integer sqrt intrinsic destination cannot use a high-byte register",
                ));
            }
            (Some(dst_register.as_str()), false)
        }
        Operand::Register(_) => {
            return Err(String::from(
                "Integer sqrt intrinsic destination must be an integer register or memory operand",
            ));
        }
        dst if is_memory_operand(dst, stack) && !matches!(dst, Operand::StringProperty { .. }) => {
            if is_float_memory_operand(dst, strings, stack)? {
                return Err(String::from(
                    "Integer sqrt intrinsic destination must be an integer memory operand",
                ));
            }
            let Some(dst_width) = operand_width(dst, strings, label_name, stack)? else {
                return Err(String::from(
                    "Integer sqrt intrinsic memory destination must have a known width",
                ));
            };
            if dst_width != expected {
                return Err(format!(
                    "Integer sqrt intrinsic destination must be {}-bit, found {}-bit memory operand",
                    expected.bits(),
                    dst_width.bits()
                ));
            }
            (None, true)
        }
        _ => {
            return Err(String::from(
                "Integer sqrt intrinsic destination must be an integer register or memory operand",
            ));
        }
    };

    validate_integer_min_max_operand(
        "Integer sqrt intrinsic operand",
        src,
        expected,
        strings,
        label_name,
        stack,
    )?;

    if let Some(value) = immediate_value(src, strings, label_name, stack) {
        let maximum = if is_signed_integer_width(width) {
            signed_width_max(expected)
        } else {
            unsigned_width_max(expected)
        };
        if value < 0 || value > maximum {
            return Err(if is_signed_integer_width(width) {
                String::from("Integer sqrt intrinsic signed operand must be non-negative")
            } else {
                String::from("Integer sqrt intrinsic operand must fit the unsigned width")
            });
        }
    }

    let mut scratch =
        ["r10", "r11", "r8", "r9"]
            .into_iter()
            .filter(|register| match dst_register {
                Some(dst_register) => !same_register_family(register, dst_register),
                None => !operand_address_uses_register_family(dst, register),
            });
    let narrow_destination =
        dst_register.is_some() && matches!(expected, Width::Bits8 | Width::Bits16);
    let accumulator = if memory_destination || narrow_destination {
        scratch.next().ok_or_else(|| {
            String::from("Integer sqrt intrinsic has no accumulator scratch register")
        })?
    } else {
        dst_register.expect("register destination")
    };
    let base = scratch
        .next()
        .ok_or_else(|| String::from("Integer sqrt intrinsic has no base scratch register"))?;
    let bit = scratch
        .next()
        .ok_or_else(|| String::from("Integer sqrt intrinsic has no bit scratch register"))?;
    let sum = scratch
        .next()
        .ok_or_else(|| String::from("Integer sqrt intrinsic has no temporary scratch register"))?;

    emit_integer_sqrt_source_load(asm, base, src, expected, strings, label_name, stack)?;

    let dst_accumulator = register_alias(accumulator, Width::Bits64)?;
    let bit_value = 1u64 << (expected.bits() - 2);
    let loop_label = format!(".L.__subsea.{label_name}.sqrt_{}_loop", asm.len());
    let find_bit_label = format!(".L.__subsea.{label_name}.sqrt_{}_find_bit", asm.len());
    let no_subtract_label = format!(".L.__subsea.{label_name}.sqrt_{}_no_subtract", asm.len());
    let negative_label = format!(".L.__subsea.{label_name}.sqrt_{}_negative", asm.len());
    let after_negative_label =
        format!(".L.__subsea.{label_name}.sqrt_{}_after_negative", asm.len());
    let done_label = format!(".L.__subsea.{label_name}.sqrt_{}_done", asm.len());

    if is_signed_integer_width(width) {
        asm.push_str(&format!("  bt {base}, {}\n", expected.bits() - 1));
        asm.push_str(&format!("  jc {negative_label}\n"));
    }
    asm.push_str(&format!("  xor {dst_accumulator}, {dst_accumulator}\n"));
    asm.push_str(&format!("  mov {bit}, {bit_value}\n"));
    asm.push_str(&format!("{find_bit_label}:\n"));
    asm.push_str(&format!("  cmp {base}, {bit}\n"));
    asm.push_str(&format!("  jae {loop_label}\n"));
    asm.push_str(&format!("  shr {bit}, 2\n"));
    asm.push_str(&format!("  test {bit}, {bit}\n"));
    asm.push_str(&format!("  jne {find_bit_label}\n"));
    asm.push_str(&format!("  jmp {done_label}\n"));
    asm.push_str(&format!("{loop_label}:\n"));
    asm.push_str(&format!("  test {bit}, {bit}\n"));
    asm.push_str(&format!("  je {done_label}\n"));
    asm.push_str(&format!("  lea {sum}, [{dst_accumulator} + {bit}]\n"));
    asm.push_str(&format!("  cmp {base}, {sum}\n"));
    asm.push_str(&format!("  jb {no_subtract_label}\n"));
    asm.push_str(&format!("  sub {base}, {sum}\n"));
    asm.push_str(&format!("  shr {dst_accumulator}, 1\n"));
    asm.push_str(&format!("  add {dst_accumulator}, {bit}\n"));
    asm.push_str(&format!("  shr {bit}, 2\n"));
    asm.push_str(&format!("  jmp {loop_label}\n"));
    asm.push_str(&format!("{no_subtract_label}:\n"));
    asm.push_str(&format!("  shr {dst_accumulator}, 1\n"));
    asm.push_str(&format!("  shr {bit}, 2\n"));
    asm.push_str(&format!("  jmp {loop_label}\n"));
    asm.push_str(&format!("{done_label}:\n"));

    if narrow_destination {
        let result = register_alias(&dst_accumulator, expected)?;
        asm.push_str(&format!("  mov {}, {result}\n", dst_register.unwrap()));
    } else if memory_destination {
        let result = Operand::Register(register_alias(&dst_accumulator, expected)?);
        emit_copy_instruction(asm, &result, dst, strings, label_name, stack)?;
    }
    if is_signed_integer_width(width) {
        asm.push_str(&format!("  jmp {after_negative_label}\n"));
        asm.push_str(&format!("{negative_label}:\n"));
        asm.push_str("  ud2\n");
        asm.push_str(&format!("{after_negative_label}:\n"));
    }

    Ok(())
}

fn emit_integer_sqrt_source_load(
    asm: &mut String,
    base: &str,
    src: &Operand,
    width: Width,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if operand_uses_high_byte_register(src) {
        return Err(String::from(
            "Integer sqrt intrinsic cannot combine high-byte registers with extended scratch registers",
        ));
    }

    let emitted = emit_operand(src, strings, label_name, stack)?;
    match width {
        Width::Bits64 => asm.push_str(&format!("  mov {base}, {emitted}\n")),
        Width::Bits32 => {
            let source = match src {
                Operand::Register(register) => register_alias(register, Width::Bits32)?,
                _ => emitted,
            };
            asm.push_str(&format!("  mov {base}d, {source}\n"));
        }
        Width::Bits16 | Width::Bits8 => {
            if immediate_value(src, strings, label_name, stack).is_some() {
                asm.push_str(&format!("  mov {base}, {emitted}\n"));
            } else {
                asm.push_str(&format!("  movzx {base}, {emitted}\n"));
            }
        }
    }
    Ok(())
}

fn unsigned_width_max(width: Width) -> i128 {
    match width {
        Width::Bits8 => u8::MAX as i128,
        Width::Bits16 => u16::MAX as i128,
        Width::Bits32 => u32::MAX as i128,
        Width::Bits64 => u64::MAX as i128,
    }
}

fn signed_width_max(width: Width) -> i128 {
    match width {
        Width::Bits8 => i8::MAX as i128,
        Width::Bits16 => i16::MAX as i128,
        Width::Bits32 => i32::MAX as i128,
        Width::Bits64 => i64::MAX as i128,
    }
}

fn is_signed_integer_width(width: MemoryWidth) -> bool {
    matches!(
        width,
        MemoryWidth::I8 | MemoryWidth::I16 | MemoryWidth::I32 | MemoryWidth::I64
    )
}

fn emit_float_rounding_intrinsic(
    asm: &mut String,
    dst: &Operand,
    op: IntrinsicOp,
    width: MemoryWidth,
    src: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let memory_destination =
        validate_float_intrinsic_destination(intrinsic_op_name(op), dst, width, strings, stack)?;
    validate_float_math_operand(
        &format!("{} operand", intrinsic_op_name(op)),
        src,
        width,
        strings,
        label_name,
        stack,
    )?;

    let dst_register = if memory_destination {
        "xmm15"
    } else {
        let Operand::Register(dst_register) = dst else {
            unreachable!()
        };
        dst_register.as_str()
    };
    let src = emit_float_operand(src, width, strings, label_name, stack)?;
    asm.push_str(&format!(
        "  {} {dst_register}, {src}, {}\n",
        float_rounding_opcode(width),
        float_rounding_mode(op)
    ));

    if memory_destination {
        emit_float_copy_instruction(
            asm,
            &Operand::Register(String::from("xmm15")),
            dst,
            width,
            strings,
            label_name,
            stack,
        )?;
    }

    Ok(())
}

fn emit_float_min_max_intrinsic(
    asm: &mut String,
    dst: &Operand,
    op: IntrinsicOp,
    width: MemoryWidth,
    lhs: &Operand,
    rhs: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let memory_destination =
        validate_float_intrinsic_destination(intrinsic_op_name(op), dst, width, strings, stack)?;
    validate_float_math_operand(
        "Floating-point intrinsic left operand",
        lhs,
        width,
        strings,
        label_name,
        stack,
    )?;
    validate_float_math_operand(
        "Floating-point intrinsic right operand",
        rhs,
        width,
        strings,
        label_name,
        stack,
    )?;

    let target = if memory_destination {
        Operand::Register(String::from("xmm15"))
    } else {
        dst.clone()
    };

    if lhs != &target {
        emit_float_copy_instruction(asm, lhs, &target, width, strings, label_name, stack)?;
    }

    let Operand::Register(dst_register) = &target else {
        unreachable!()
    };
    let rhs = emit_float_operand(rhs, width, strings, label_name, stack)?;
    asm.push_str(&format!(
        "  {} {dst_register}, {rhs}\n",
        float_min_max_opcode(op, width)
    ));

    if memory_destination {
        emit_float_copy_instruction(asm, &target, dst, width, strings, label_name, stack)?;
    }

    Ok(())
}

fn emit_integer_min_max_intrinsic(
    asm: &mut String,
    dst: &Operand,
    op: IntrinsicOp,
    width: MemoryWidth,
    lhs: &Operand,
    rhs: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_integer_min_max_intrinsic(dst, width, lhs, rhs, strings, label_name, stack)?;

    let intrinsic_width = memory_width_bits(width);
    let memory_destination = !matches!(dst, Operand::Register(_));
    let result_register = if memory_destination {
        integer_memory_result_register(dst, rhs)?
    } else {
        match dst {
            Operand::Register(register) => register.clone(),
            _ => unreachable!(),
        }
    };
    let result = Operand::Register(register_alias(&result_register, intrinsic_width)?);
    emit_copy_instruction(asm, lhs, &result, strings, label_name, stack)?;
    let rhs = integer_min_max_rhs(rhs, intrinsic_width, strings, label_name, stack)?;
    let dst_operand = emit_operand(&result, strings, label_name, stack)?;
    let rhs_operand = emit_operand(&rhs, strings, label_name, stack)?;
    let keep_label = format!(
        ".L.__subsea.{label_name}.{}_{}_keep",
        intrinsic_op_name(op),
        asm.len()
    );

    asm.push_str(&format!("  cmp {dst_operand}, {rhs_operand}\n"));
    asm.push_str(&format!(
        "  {} {keep_label}\n",
        integer_min_max_keep_jump(op, width)
    ));
    asm.push_str(&format!("  mov {dst_operand}, {rhs_operand}\n"));
    asm.push_str(&format!("{keep_label}:\n"));

    if memory_destination {
        emit_copy_instruction(asm, &result, dst, strings, label_name, stack)?;
    }

    Ok(())
}

fn integer_memory_result_register(dst: &Operand, rhs: &Operand) -> Result<String, String> {
    ["r10", "r11", "r8", "r9"]
        .into_iter()
        .find(|register| {
            !operand_address_uses_register_family(dst, register)
                && !operand_uses_register_family(rhs, register)
        })
        .map(String::from)
        .ok_or_else(|| {
            String::from("Integer min/max intrinsic has no available result scratch register")
        })
}

fn validate_float_intrinsic_destination(
    name: &str,
    dst: &Operand,
    width: MemoryWidth,
    strings: &StringTable,
    stack: &StackFrame,
) -> Result<bool, String> {
    match dst {
        Operand::Register(register) if is_xmm_register(register) => Ok(false),
        dst if is_memory_operand(dst, stack)
            && !matches!(dst, Operand::StringProperty { .. })
            && is_float_memory_operand(dst, strings, stack)? =>
        {
            if resolve_memory_width_for_float_destination(dst, strings, stack)? != Some(width) {
                return Err(format!(
                    "{name} floating-point intrinsic destination width must match {width:?}"
                ));
            }
            Ok(true)
        }
        _ => Err(format!(
            "{name} floating-point intrinsic destination must be an XMM register or floating-point memory operand"
        )),
    }
}

fn resolve_memory_width_for_float_destination(
    dst: &Operand,
    strings: &StringTable,
    stack: &StackFrame,
) -> Result<Option<MemoryWidth>, String> {
    match dst {
        Operand::Dereference { address, width } => resolve_memory_width(address, *width, strings),
        Operand::Ident(name) => Ok(stack_scalar_slot(stack, name).map(|(_, width)| width)),
        _ => Ok(None),
    }
}

fn validate_integer_min_max_intrinsic(
    dst: &Operand,
    width: MemoryWidth,
    lhs: &Operand,
    rhs: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let expected = memory_width_bits(width);
    let dst_width = match dst {
        Operand::Register(dst_register) if !is_xmm_register(dst_register) => {
            register_width(dst_register).ok_or_else(|| {
                String::from("Integer min/max intrinsic destination must be a register")
            })?
        }
        Operand::Register(_) => {
            return Err(String::from(
                "Integer min/max intrinsic destination must be an integer register",
            ));
        }
        dst if is_memory_operand(dst, stack) && !matches!(dst, Operand::StringProperty { .. }) => {
            if is_float_memory_operand(dst, strings, stack)? {
                return Err(String::from(
                    "Integer min/max intrinsic destination must be integer memory",
                ));
            }
            operand_width(dst, strings, label_name, stack)?.ok_or_else(|| {
                String::from("Integer min/max intrinsic memory destination must have a known width")
            })?
        }
        _ => {
            return Err(String::from(
                "Integer min/max intrinsic destination must be an integer register or memory operand",
            ));
        }
    };
    if dst_width != expected {
        return Err(format!(
            "Integer min/max intrinsic destination must be {}-bit, found {}-bit operand",
            expected.bits(),
            dst_width.bits()
        ));
    }

    validate_integer_min_max_operand(
        "Integer min/max intrinsic left operand",
        lhs,
        expected,
        strings,
        label_name,
        stack,
    )?;
    validate_integer_min_max_operand(
        "Integer min/max intrinsic right operand",
        rhs,
        expected,
        strings,
        label_name,
        stack,
    )?;

    if operand_uses_high_byte_register(lhs) && operand_uses_extended_register(dst) {
        return Err(String::from(
            "Integer min/max intrinsic cannot combine high-byte registers ah, bh, ch, or dh with extended registers",
        ));
    }

    if operand_uses_high_byte_register(rhs) && operand_uses_extended_register(dst) {
        return Err(String::from(
            "Integer min/max intrinsic cannot combine high-byte registers ah, bh, ch, or dh with extended registers",
        ));
    }

    Ok(())
}

fn validate_integer_min_max_operand(
    name: &str,
    operand: &Operand,
    expected: Width,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if operand_uses_xmm_register(operand) || is_float_memory_operand(operand, strings, stack)? {
        return Err(format!("{name} must be an integer operand"));
    }

    if matches!(
        operand,
        Operand::Pointer(_)
            | Operand::AddressOf(_)
            | Operand::StringProperty { .. }
            | Operand::Converted { .. }
            | Operand::Cast { .. }
            | Operand::FloatLiteral(_)
    ) {
        return Err(format!("{name} must be an integer operand"));
    }

    if let Some(width) = operand_width(operand, strings, label_name, stack)?
        && width != expected
    {
        return Err(format!(
            "{name} must be {}-bit, found {}-bit operand",
            expected.bits(),
            width.bits()
        ));
    }

    Ok(())
}

fn integer_min_max_rhs(
    rhs: &Operand,
    width: Width,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<Operand, String> {
    match rhs {
        Operand::Register(register) if register_width(register).is_some_and(|rhs| rhs != width) => {
            Ok(Operand::Register(register_alias(register, width)?))
        }
        _ => {
            if is_immediate_operand(rhs, strings, label_name, stack) {
                return Ok(rhs.clone());
            }

            Ok(rhs.clone())
        }
    }
}

fn integer_min_max_keep_jump(op: IntrinsicOp, width: MemoryWidth) -> &'static str {
    match (op, width) {
        (
            IntrinsicOp::Min,
            MemoryWidth::I8 | MemoryWidth::I16 | MemoryWidth::I32 | MemoryWidth::I64,
        ) => "jle",
        (
            IntrinsicOp::Min,
            MemoryWidth::U8 | MemoryWidth::U16 | MemoryWidth::U32 | MemoryWidth::U64,
        ) => "jbe",
        (
            IntrinsicOp::Max,
            MemoryWidth::I8 | MemoryWidth::I16 | MemoryWidth::I32 | MemoryWidth::I64,
        ) => "jge",
        (
            IntrinsicOp::Max,
            MemoryWidth::U8 | MemoryWidth::U16 | MemoryWidth::U32 | MemoryWidth::U64,
        ) => "jae",
        _ => unreachable!(),
    }
}

fn float_sqrt_opcode(width: MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::F32 => "sqrtss",
        MemoryWidth::F64 => "sqrtsd",
        _ => unreachable!(),
    }
}

fn float_rounding_opcode(width: MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::F32 => "roundss",
        MemoryWidth::F64 => "roundsd",
        _ => unreachable!(),
    }
}

fn float_rounding_mode(op: IntrinsicOp) -> u8 {
    match op {
        IntrinsicOp::Round => 0,
        IntrinsicOp::Floor => 1,
        IntrinsicOp::Ceil => 2,
        IntrinsicOp::Trunc => 3,
        _ => unreachable!(),
    }
}

fn float_min_max_opcode(op: IntrinsicOp, width: MemoryWidth) -> &'static str {
    match (op, width) {
        (IntrinsicOp::Min, MemoryWidth::F32) => "minss",
        (IntrinsicOp::Min, MemoryWidth::F64) => "minsd",
        (IntrinsicOp::Max, MemoryWidth::F32) => "maxss",
        (IntrinsicOp::Max, MemoryWidth::F64) => "maxsd",
        _ => unreachable!(),
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

fn emit_float_binary_assignment(
    asm: &mut String,
    dst: &AssignmentTarget,
    width: MemoryWidth,
    op: FloatMathOp,
    lhs: &Operand,
    rhs: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_float_width(width)?;

    let dst = assignment_operand_target(dst)?;
    emit_float_binary_operand_assignment(asm, dst, width, op, lhs, rhs, strings, label_name, stack)
}

fn emit_float_binary_operand_assignment(
    asm: &mut String,
    dst: &Operand,
    width: MemoryWidth,
    op: FloatMathOp,
    lhs: &Operand,
    rhs: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_float_width(width)?;

    let Operand::Register(dst_register) = dst else {
        return Err(String::from(
            "Floating-point arithmetic destination must be an XMM register",
        ));
    };

    if !is_xmm_register(dst_register) {
        return Err(String::from(
            "Floating-point arithmetic destination must be an XMM register",
        ));
    }

    validate_float_math_operand(
        "Floating-point arithmetic left operand",
        lhs,
        width,
        strings,
        label_name,
        stack,
    )?;
    validate_float_math_operand(
        "Floating-point arithmetic right operand",
        rhs,
        width,
        strings,
        label_name,
        stack,
    )?;

    if rhs == dst {
        if matches!(op, FloatMathOp::Add | FloatMathOp::Multiply) {
            let rhs = emit_float_operand(lhs, width, strings, label_name, stack)?;
            asm.push_str(&format!(
                "  {} {dst_register}, {rhs}\n",
                float_math_opcode(op, width)
            ));
            return Ok(());
        }

        return Err(String::from(
            "Non-commutative floating-point assignment destination cannot also be the right operand",
        ));
    }

    if lhs != dst {
        emit_float_copy_instruction(asm, lhs, dst, width, strings, label_name, stack)?;
    }

    let rhs = emit_float_operand(rhs, width, strings, label_name, stack)?;
    asm.push_str(&format!(
        "  {} {dst_register}, {rhs}\n",
        float_math_opcode(op, width)
    ));

    Ok(())
}

fn emit_float_copy_instruction(
    asm: &mut String,
    src: &Operand,
    dst: &Operand,
    width: MemoryWidth,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let src = emit_float_operand(src, width, strings, label_name, stack)?;
    let dst = emit_operand(dst, strings, label_name, stack)?;
    asm.push_str(&format!(
        "  {} {dst}, {src}\n",
        float_move_opcode_for_width(width)?
    ));

    Ok(())
}

fn emit_float_operand(
    operand: &Operand,
    width: MemoryWidth,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<String, String> {
    match operand {
        Operand::FloatLiteral(value) => {
            let binding = strings
                .float_literals
                .get(&(label_name.to_string(), width, value.clone()))
                .ok_or_else(|| String::from("Internal error: missing float literal"))?;

            Ok(format!(
                "{} ptr [rip + {}]",
                binding.width.ptr(),
                binding.asm_label
            ))
        }
        Operand::Ident(name) if stack_scalar_slot(stack, name).is_none() => {
            let binding = strings
                .float_bindings
                .get(&(label_name.to_string(), name.clone()))
                .ok_or_else(|| format!("Unknown float binding {name:?} in label {label_name:?}"))?;

            Ok(format!(
                "{} ptr [rip + {}]",
                binding.width.ptr(),
                binding.asm_label
            ))
        }
        _ => emit_operand(operand, strings, label_name, stack),
    }
}

fn validate_float_math_operand(
    name: &str,
    operand: &Operand,
    width: MemoryWidth,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match operand {
        Operand::Converted { .. } | Operand::Cast { .. } => Err(format!(
            "{name} cannot use integer width conversion in floating-point math"
        )),
        Operand::AddressOf(_) => Err(format!("{name} cannot be an address-of operand")),
        Operand::Register(register) if is_xmm_register(register) => Ok(()),
        Operand::FloatLiteral(value) => validate_float_literal(value, width),
        Operand::Ident(binding) if stack_scalar_slot(stack, binding).is_some() => {
            match stack_scalar_slot(stack, binding) {
                Some((_, stack_width)) if stack_width == width && stack_width.is_float() => Ok(()),
                Some((_, MemoryWidth::F32 | MemoryWidth::F64)) => Err(format!(
                    "{name} width must match the floating-point operator width"
                )),
                Some(_) => Err(format!(
                    "{name} must be an XMM register or floating-point memory operand"
                )),
                None => unreachable!(),
            }
        }
        Operand::Ident(binding) => {
            match strings
                .float_bindings
                .get(&(label_name.to_string(), binding.clone()))
            {
                Some(float) if float.width == width => Ok(()),
                Some(_) => Err(format!(
                    "{name} width must match the floating-point operator width"
                )),
                None => Err(format!("{name} cannot be a const or stack binding for now")),
            }
        }
        Operand::Dereference {
            address,
            width: memory_width,
        } => match resolve_memory_width(address, *memory_width, strings)? {
            Some(resolved_width) if resolved_width == width => Ok(()),
            Some(MemoryWidth::F32 | MemoryWidth::F64) => Err(format!(
                "{name} width must match the floating-point operator width"
            )),
            Some(_) => Err(format!(
                "{name} must be an XMM register or floating-point memory operand"
            )),
            None => Err(format!(
                "{name} memory operand requires an explicit f32 or f64 width"
            )),
        },
        Operand::Immediate(_) => Err(format!(
            "{name} cannot be an immediate value; use a floating-point memory operand for now"
        )),
        Operand::StringProperty { .. } => Err(format!("{name} cannot be a string property")),
        Operand::Pointer(_) => Err(format!("{name} cannot be an address-of operand")),
        Operand::Register(register) => Err(format!(
            "{name} must be an XMM register, found integer register {register}"
        )),
    }
}

fn float_math_opcode(op: FloatMathOp, width: MemoryWidth) -> &'static str {
    match (op, width) {
        (FloatMathOp::Add, MemoryWidth::F32) => "addss",
        (FloatMathOp::Add, MemoryWidth::F64) => "addsd",
        (FloatMathOp::Divide, MemoryWidth::F32) => "divss",
        (FloatMathOp::Divide, MemoryWidth::F64) => "divsd",
        (FloatMathOp::Multiply, MemoryWidth::F32) => "mulss",
        (FloatMathOp::Multiply, MemoryWidth::F64) => "mulsd",
        (FloatMathOp::Subtract, MemoryWidth::F32) => "subss",
        (FloatMathOp::Subtract, MemoryWidth::F64) => "subsd",
        _ => unreachable!(),
    }
}

fn validate_binary_assignment_does_not_clobber_rhs_address(
    dst: &Operand,
    rhs: &Operand,
) -> Result<(), String> {
    let Operand::Register(dst_register) = dst else {
        return Ok(());
    };

    if operand_address_uses_register_family(rhs, dst_register) {
        return Err(format!(
            "Binary assignment destination {dst_register} cannot be used in the right operand address"
        ));
    }

    Ok(())
}

fn validate_shift_assignment_does_not_clobber_count(
    dst: &Operand,
    count: &Operand,
) -> Result<(), String> {
    let Operand::Register(dst_register) = dst else {
        return Ok(());
    };

    if operand_uses_register_family(count, dst_register) {
        return Err(format!(
            "Shift assignment destination {dst_register} cannot also be used as the count operand"
        ));
    }

    Ok(())
}

fn assignment_operand_target(dst: &AssignmentTarget) -> Result<&Operand, String> {
    match dst {
        AssignmentTarget::Operand(operand) => Ok(operand),
        AssignmentTarget::RegisterPair(_) => Err(String::from(
            "Register-pair assignment requires a widened multiply right side",
        )),
    }
}

fn validate_wide_math_target(operation: &str, dst: &AssignmentTarget) -> Result<(), String> {
    match dst {
        AssignmentTarget::RegisterPair(RegisterPair { high, low })
            if high == "rdx" && low == "rax" =>
        {
            Ok(())
        }
        AssignmentTarget::RegisterPair(RegisterPair { high, low }) => Err(format!(
            "{operation} destination must be rdx:rax, found {high}:{low}"
        )),
        AssignmentTarget::Operand(_) => Err(String::from(
            "Widened math destination must be the register pair rdx:rax",
        )),
    }
}

fn validate_wide_math_operand(
    name: &str,
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(operand, Operand::Pointer(_)) {
        return Err(format!("{name} cannot be an address-of operand"));
    }

    if let Some(width) = operand_width(operand, strings, label_name, stack)?
        && width != Width::Bits64
    {
        return Err(format!(
            "{name} must be 64-bit, found {}-bit operand",
            width.bits()
        ));
    }

    Ok(())
}

fn emit_copy_instruction(
    asm: &mut String,
    src: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if src == dst
        && !matches!(
            src,
            Operand::Converted { .. }
                | Operand::Cast { .. }
                | Operand::Pointer(_)
                | Operand::AddressOf(_)
        )
    {
        return Ok(());
    }

    if let Operand::Converted {
        operand,
        conversion,
    } = src
    {
        emit_width_conversion_copy(asm, operand, *conversion, dst, strings, label_name, stack)
    } else if let Operand::Cast { operand, width } = src {
        emit_numeric_cast_copy(asm, operand, *width, dst, strings, label_name, stack)
    } else if emit_truncating_copy(asm, src, dst, strings, label_name, stack)? {
        Ok(())
    } else if let Some(opcode) = float_move_opcode(src, dst, strings, stack)? {
        let src = emit_operand(src, strings, label_name, stack)?;
        let dst = emit_operand(dst, strings, label_name, stack)?;
        asm.push_str(&format!("  {opcode} {dst}, {src}\n"));

        Ok(())
    } else if operand_uses_xmm_register(src) || operand_uses_xmm_register(dst) {
        Err(String::from(
            "XMM moves require one XMM register and one explicitly f32 or f64 memory operand",
        ))
    } else if is_float_memory_operand(src, strings, stack)?
        || is_float_memory_operand(dst, strings, stack)?
    {
        Err(String::from(
            "Floating-point memory operands require an XMM register source or destination",
        ))
    } else if let Operand::Pointer(name) = src {
        validate_address_copy_dst(dst)?;

        let dst = emit_operand(dst, strings, label_name, stack)?;
        asm.push_str(&format!("  lea {dst}, [rip + {name}]\n"));

        Ok(())
    } else if let Operand::AddressOf(address) = src {
        validate_address_copy_dst(dst)?;

        let dst = emit_operand(dst, strings, label_name, stack)?;
        let address = emit_address(address);
        asm.push_str(&format!("  lea {dst}, [{address}]\n"));

        Ok(())
    } else {
        emit_binary_instruction(asm, "mov", src, dst, strings, label_name, stack)
    }
}

fn emit_width_conversion_copy(
    asm: &mut String,
    src: &Operand,
    conversion: WidthConversion,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let Operand::Register(dst_register) = dst else {
        return Err(String::from(
            "Width conversion destination must be an integer register",
        ));
    };

    if is_xmm_register(dst_register) {
        return Err(String::from(
            "Width conversion destination must be an integer register",
        ));
    }

    validate_width_conversion_source(src, strings, label_name, stack)?;

    let dst_width = register_width(dst_register)
        .ok_or_else(|| String::from("Width conversion destination must be an integer register"))?;
    let src_width = operand_width(src, strings, label_name, stack)?
        .ok_or_else(|| String::from("Width conversion source must have a known integer width"))?;

    if src_width.bits() >= dst_width.bits() {
        return Err(format!(
            "Width conversion source must be narrower than destination, found {}-bit source and {}-bit destination",
            src_width.bits(),
            dst_width.bits()
        ));
    }

    if operand_uses_high_byte_register(src) && is_extended_register(dst_register) {
        return Err(String::from(
            "Width conversion cannot combine high-byte registers ah, bh, ch, or dh with extended registers",
        ));
    }

    let src = emit_operand(src, strings, label_name, stack)?;
    match (conversion, src_width, dst_width) {
        (WidthConversion::ZeroExtend, Width::Bits32, Width::Bits64) => {
            let dst = register_alias(dst_register, Width::Bits32)?;
            asm.push_str(&format!("  mov {dst}, {src}\n"));
        }
        (WidthConversion::ZeroExtend, _, _) => {
            let dst = emit_operand(dst, strings, label_name, stack)?;
            asm.push_str(&format!("  movzx {dst}, {src}\n"));
        }
        (WidthConversion::SignExtend, Width::Bits32, Width::Bits64) => {
            let dst = emit_operand(dst, strings, label_name, stack)?;
            asm.push_str(&format!("  movsxd {dst}, {src}\n"));
        }
        (WidthConversion::SignExtend, _, _) => {
            let dst = emit_operand(dst, strings, label_name, stack)?;
            asm.push_str(&format!("  movsx {dst}, {src}\n"));
        }
    }

    Ok(())
}

fn emit_numeric_cast_copy(
    asm: &mut String,
    src: &Operand,
    target_width: MemoryWidth,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(target_width, MemoryWidth::Ptr) {
        return Err(String::from(
            "Numeric casts cannot use ptr as a target width",
        ));
    }

    if target_width.is_float() {
        return emit_int_to_float_cast(asm, src, target_width, dst, strings, label_name, stack);
    }

    emit_float_to_int_cast(asm, src, target_width, dst, strings, label_name, stack)
}

fn emit_int_to_float_cast(
    asm: &mut String,
    src: &Operand,
    target_width: MemoryWidth,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let Operand::Register(dst_register) = dst else {
        return Err(String::from(
            "Integer-to-float cast destination must be an XMM register",
        ));
    };

    if !is_xmm_register(dst_register) {
        return Err(String::from(
            "Integer-to-float cast destination must be an XMM register",
        ));
    }

    validate_integer_cast_source(src, strings, label_name, stack)?;
    let src_width = operand_width(src, strings, label_name, stack)?
        .ok_or_else(|| String::from("Integer-to-float cast source must have a known width"))?;

    let opcode = match target_width {
        MemoryWidth::F32 => "cvtsi2ss",
        MemoryWidth::F64 => "cvtsi2sd",
        _ => unreachable!(),
    };
    let src = cast_integer_operand_for_cvtsi(src, src_width, strings, label_name, stack)?;
    asm.push_str(&format!("  {opcode} {dst_register}, {src}\n"));

    Ok(())
}

fn emit_float_to_int_cast(
    asm: &mut String,
    src: &Operand,
    target_width: MemoryWidth,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let Operand::Register(dst_register) = dst else {
        return Err(String::from(
            "Float-to-integer cast destination must be an integer register",
        ));
    };

    if is_xmm_register(dst_register) {
        return Err(String::from(
            "Float-to-integer cast destination must be an integer register",
        ));
    }

    let source_width = resolve_cast_float_source_width(src, strings, label_name, stack)?;
    validate_integer_cast_width(target_width)?;

    let dst_width = memory_width_bits(target_width);
    let cast_dst = match dst_width {
        Width::Bits64 => dst_register.clone(),
        Width::Bits32 => register_alias(dst_register, Width::Bits32)?,
        Width::Bits16 | Width::Bits8 => String::from("r11d"),
    };
    let opcode = match source_width {
        MemoryWidth::F32 => "cvttss2si",
        MemoryWidth::F64 => "cvttsd2si",
        _ => unreachable!(),
    };
    let src = emit_float_operand(src, source_width, strings, label_name, stack)?;
    asm.push_str(&format!("  {opcode} {cast_dst}, {src}\n"));

    if matches!(dst_width, Width::Bits16 | Width::Bits8) {
        let narrowed = register_alias("r11", dst_width)?;
        let dst = emit_operand(dst, strings, label_name, stack)?;
        asm.push_str(&format!("  mov {dst}, {narrowed}\n"));
    }

    Ok(())
}

fn validate_integer_cast_source(
    src: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(
        src,
        Operand::Pointer(_) | Operand::AddressOf(_) | Operand::FloatLiteral(_)
    ) || operand_uses_xmm_register(src)
        || is_float_memory_operand(src, strings, stack)?
    {
        return Err(String::from(
            "Integer-to-float cast source must be an integer operand",
        ));
    }

    if matches!(src, Operand::Immediate(_)) {
        return Err(String::from(
            "Integer-to-float cast source must be a register or memory operand",
        ));
    }

    let Some(width) = operand_width(src, strings, label_name, stack)? else {
        return Err(String::from(
            "Integer-to-float cast source must have a known width",
        ));
    };

    if matches!(width, Width::Bits8 | Width::Bits16) {
        return Err(String::from(
            "Integer-to-float cast source must be 32-bit or 64-bit for now",
        ));
    }

    Ok(())
}

fn cast_integer_operand_for_cvtsi(
    src: &Operand,
    width: Width,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<String, String> {
    match src {
        Operand::Register(register) if width == Width::Bits32 => {
            register_alias(register, Width::Bits32)
        }
        _ => emit_operand(src, strings, label_name, stack),
    }
}

fn resolve_cast_float_source_width(
    src: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<MemoryWidth, String> {
    if let Some(width) = operand_float_width(src, strings, label_name, stack)? {
        return Ok(width);
    }

    if matches!(src, Operand::Register(register) if is_xmm_register(register)) {
        return Err(String::from(
            "Float-to-integer cast from an XMM register needs an explicit source width; cast a typed memory operand or use f32/f64 arithmetic first",
        ));
    }

    Err(String::from(
        "Float-to-integer cast source must be a floating-point operand",
    ))
}

fn validate_integer_cast_width(width: MemoryWidth) -> Result<(), String> {
    match width {
        MemoryWidth::I8
        | MemoryWidth::I16
        | MemoryWidth::I32
        | MemoryWidth::I64
        | MemoryWidth::U8
        | MemoryWidth::U16
        | MemoryWidth::U32
        | MemoryWidth::U64 => Ok(()),
        MemoryWidth::F32 | MemoryWidth::F64 | MemoryWidth::Ptr => Err(String::from(
            "Float-to-integer cast target must be an integer width",
        )),
    }
}

fn emit_truncating_copy(
    asm: &mut String,
    src: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<bool, String> {
    let Operand::Register(src_register) = src else {
        return Ok(false);
    };

    let Some(src_width) = register_width(src_register) else {
        return Ok(false);
    };
    let Some(dst_width) = operand_width(dst, strings, label_name, stack)? else {
        return Ok(false);
    };

    if src_width.bits() <= dst_width.bits() {
        return Ok(false);
    }

    validate_binary_operands("mov", src, dst, strings, label_name, stack)?;
    let dst = emit_operand(dst, strings, label_name, stack)?;
    let src = register_alias(src_register, dst_width)?;
    asm.push_str(&format!("  mov {dst}, {src}\n"));

    Ok(true)
}

fn validate_binary_operands(
    opcode: &str,
    src: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if opcode != "mov"
        && (operand_uses_xmm_register(src)
            || operand_uses_xmm_register(dst)
            || is_float_memory_operand(src, strings, stack)?
            || is_float_memory_operand(dst, strings, stack)?)
    {
        return Err(format!(
            "{opcode} does not support floating-point operands yet"
        ));
    }

    if matches!(
        dst,
        Operand::Immediate(_)
            | Operand::Pointer(_)
            | Operand::AddressOf(_)
            | Operand::StringProperty { .. }
            | Operand::Converted { .. }
            | Operand::Cast { .. }
    ) || matches!(dst, Operand::Ident(name) if stack_scalar_slot(stack, name).is_none())
    {
        return Err(format!(
            "{opcode} destination must be a register or memory operand"
        ));
    }

    if matches!(src, Operand::Pointer(_)) {
        return Err(format!("{opcode} source cannot be an address-of operand"));
    }

    if matches!(src, Operand::AddressOf(_)) {
        return Err(format!("{opcode} source cannot be an address-of operand"));
    }

    if matches!(src, Operand::Converted { .. } | Operand::Cast { .. }) {
        return Err(format!("{opcode} source cannot use conversion here"));
    }

    if matches!(src, Operand::FloatLiteral(_)) || matches!(dst, Operand::FloatLiteral(_)) {
        return Err(format!(
            "{opcode} cannot use floating-point literal operands"
        ));
    }

    if operand_uses_high_byte_register(src) && operand_uses_extended_register(dst) {
        return Err(format!(
            "{opcode} cannot combine high-byte registers ah, bh, ch, or dh with extended registers"
        ));
    }

    if operand_uses_high_byte_register(dst) && operand_uses_extended_register(src) {
        return Err(format!(
            "{opcode} cannot combine high-byte registers ah, bh, ch, or dh with extended registers"
        ));
    }

    if is_memory_operand(src, stack) && is_memory_operand(dst, stack) {
        return Err(format!(
            "{opcode} cannot use memory for both source and destination"
        ));
    }

    if opcode == "mov"
        && is_immediate_operand(src, strings, label_name, stack)
        && matches!(dst, Operand::Dereference { width: None, .. })
        && destination_width(dst, strings, stack)?.is_none()
    {
        return Err(String::from(
            "Cannot assign an immediate value directly into memory without an explicit width",
        ));
    }

    if let (Some(src_width), Some(dst_width)) = (
        operand_width(src, strings, label_name, stack)?,
        operand_width(dst, strings, label_name, stack)?,
    ) && src_width != dst_width
    {
        if opcode == "mov"
            && matches!(src, Operand::Register(_))
            && src_width.bits() > dst_width.bits()
        {
            return Ok(());
        }

        return Err(format!(
            "Cannot use {}-bit source with {}-bit destination",
            src_width.bits(),
            dst_width.bits()
        ));
    }

    if let (Some(value), Some(width)) = (
        immediate_value(src, strings, label_name, stack),
        destination_width(dst, strings, stack)?,
    ) {
        validate_immediate_range(value, width)?;
    }

    Ok(())
}

fn validate_shift_operands(
    opcode: &str,
    count: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_bitwise_unary_operand(opcode, dst, strings, stack)?;

    if let Some(value) = immediate_value(count, strings, label_name, stack) {
        if (0..=255).contains(&value) {
            return Ok(());
        }

        return Err(format!(
            "{opcode} count immediate must be between 0 and 255"
        ));
    }

    match count {
        Operand::Register(register) if register == "cl" => Ok(()),
        Operand::Register(register) => Err(format!(
            "{opcode} count must be an immediate value or cl, found register {register}"
        )),
        _ => Err(format!("{opcode} count must be an immediate value or cl")),
    }
}

fn validate_width_conversion_source(
    src: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(src, Operand::Converted { .. }) {
        return Err(String::from("Width conversions cannot be nested"));
    }

    if matches!(
        src,
        Operand::Immediate(_) | Operand::Pointer(_) | Operand::AddressOf(_)
    ) {
        return Err(String::from(
            "Width conversion source must be an integer register or memory operand",
        ));
    }

    if operand_uses_xmm_register(src) || is_float_memory_operand(src, strings, stack)? {
        return Err(String::from(
            "Width conversion source must be an integer register or memory operand",
        ));
    }

    if operand_width(src, strings, label_name, stack)?.is_none() {
        return Err(String::from(
            "Width conversion source must have a known integer width",
        ));
    }

    Ok(())
}

fn validate_boolean_assignment_destination(
    dst: &Operand,
    strings: &StringTable,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(
        dst,
        Operand::Immediate(_)
            | Operand::Pointer(_)
            | Operand::AddressOf(_)
            | Operand::StringProperty { .. }
            | Operand::FloatLiteral(_)
    ) || matches!(dst, Operand::Ident(name) if stack_scalar_slot(stack, name).is_none())
    {
        return Err(String::from(
            "Boolean assignment destination must be a register or integer memory operand",
        ));
    }

    if operand_uses_xmm_register(dst) || is_float_memory_operand(dst, strings, stack)? {
        return Err(String::from(
            "Boolean assignment destination must be an integer register or memory operand",
        ));
    }

    Ok(())
}

fn validate_boolean_movzx_width(width: Width) -> Result<(), String> {
    match width {
        Width::Bits8 => Err(String::from(
            "Internal error: 8-bit boolean register destination should use setcc directly",
        )),
        Width::Bits16 | Width::Bits32 | Width::Bits64 => Ok(()),
    }
}

fn boolean_temp_register(dst: &Operand) -> Result<&'static str, String> {
    if !operand_address_uses_register_family(dst, "r10") {
        Ok("r10")
    } else if !operand_address_uses_register_family(dst, "r11") {
        Ok("r11")
    } else {
        Err(String::from(
            "Boolean assignment destination address cannot use both r10 and r11",
        ))
    }
}

fn temp_register_for_memory_width(temp: &str, width: MemoryWidth) -> Result<String, String> {
    let suffix = match memory_width_bits(width) {
        Width::Bits8 => "b",
        Width::Bits16 => "w",
        Width::Bits32 => "d",
        Width::Bits64 => "",
    };

    if width.is_float() {
        return Err(String::from(
            "Boolean assignment destination must be integer memory",
        ));
    }

    Ok(format!("{temp}{suffix}"))
}

fn validate_bitwise_unary_operand(
    opcode: &str,
    dst: &Operand,
    strings: &StringTable,
    stack: &StackFrame,
) -> Result<(), String> {
    if operand_uses_xmm_register(dst) || is_float_memory_operand(dst, strings, stack)? {
        return Err(format!(
            "{opcode} does not support floating-point operands yet"
        ));
    }

    if matches!(
        dst,
        Operand::Immediate(_)
            | Operand::Pointer(_)
            | Operand::AddressOf(_)
            | Operand::StringProperty { .. }
            | Operand::FloatLiteral(_)
    ) || matches!(dst, Operand::Ident(name) if stack_scalar_slot(stack, name).is_none())
    {
        return Err(format!(
            "{opcode} destination must be a register or memory operand"
        ));
    }

    Ok(())
}

fn is_immediate_operand(
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> bool {
    match operand {
        Operand::Immediate(_) => true,
        Operand::Ident(name) => {
            !stack.slots.contains_key(name)
                && strings
                    .integers
                    .contains_key(&(label_name.to_string(), name.clone()))
        }
        _ => false,
    }
}

fn validate_immediate_range(value: i128, destination: ImmediateDestination) -> Result<(), String> {
    let valid = match destination {
        ImmediateDestination::Register(Width::Bits8) => {
            i8::MIN as i128 <= value && value <= u8::MAX as i128
        }
        ImmediateDestination::Register(Width::Bits16) => {
            i16::MIN as i128 <= value && value <= u16::MAX as i128
        }
        ImmediateDestination::Register(Width::Bits32) => {
            i32::MIN as i128 <= value && value <= u32::MAX as i128
        }
        ImmediateDestination::Register(Width::Bits64) => {
            i64::MIN as i128 <= value && value <= u64::MAX as i128
        }
        ImmediateDestination::Memory(MemoryWidth::I8) => {
            i8::MIN as i128 <= value && value <= i8::MAX as i128
        }
        ImmediateDestination::Memory(MemoryWidth::F32 | MemoryWidth::F64) => {
            return Err(String::from(
                "Integer immediate values cannot be assigned to floating-point memory destinations yet",
            ));
        }
        ImmediateDestination::Memory(MemoryWidth::I16) => {
            i16::MIN as i128 <= value && value <= i16::MAX as i128
        }
        ImmediateDestination::Memory(MemoryWidth::I32) => {
            i32::MIN as i128 <= value && value <= i32::MAX as i128
        }
        ImmediateDestination::Memory(MemoryWidth::I64 | MemoryWidth::U64 | MemoryWidth::Ptr) => {
            if i32::MIN as i128 <= value && value <= i32::MAX as i128 {
                true
            } else {
                return Err(format!(
                    "Immediate value {value} cannot be encoded directly into a 64-bit memory destination; move it through a 64-bit register first"
                ));
            }
        }
        ImmediateDestination::Memory(MemoryWidth::U8) => 0 <= value && value <= u8::MAX as i128,
        ImmediateDestination::Memory(MemoryWidth::U16) => 0 <= value && value <= u16::MAX as i128,
        ImmediateDestination::Memory(MemoryWidth::U32) => 0 <= value && value <= u32::MAX as i128,
    };

    if valid {
        Ok(())
    } else {
        Err(format!(
            "Immediate value {value} does not fit in {}-bit destination",
            destination.bits()
        ))
    }
}

fn validate_address_copy_dst(dst: &Operand) -> Result<(), String> {
    match dst {
        Operand::Register(register) if is_xmm_register(register) => Err(String::from(
            "Address-of labels can only be copied into 64-bit integer registers",
        )),
        Operand::Register(register) => match register_width(register) {
            Some(Width::Bits64) => Ok(()),
            Some(width) => Err(format!(
                "Address-of labels can only be copied into 64-bit registers, found {}-bit register",
                width.bits()
            )),
            None => Err(String::from(
                "Address-of labels can only be copied into 64-bit registers",
            )),
        },
        _ => Err(String::from(
            "Address-of labels can only be copied into registers for now",
        )),
    }
}

fn validate_push_operand(
    src: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(src, Operand::Pointer(_) | Operand::AddressOf(_)) {
        return Err(String::from("push source cannot be an address-of operand"));
    }

    if is_immediate_operand(src, strings, label_name, stack) {
        return Ok(());
    }

    validate_stack_width("push source", src, strings, label_name, stack)
}

fn validate_pop_operand(
    dst: &Operand,
    strings: &StringTable,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(
        dst,
        Operand::Immediate(_)
            | Operand::Pointer(_)
            | Operand::AddressOf(_)
            | Operand::StringProperty { .. }
    ) {
        return Err(String::from(
            "pop destination must be a 64-bit register or explicitly 64-bit memory operand",
        ));
    }

    validate_stack_width("pop destination", dst, strings, "", stack)
}

fn validate_stack_width(
    name: &str,
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match operand {
        Operand::Register(register) if is_xmm_register(register) => Err(format!(
            "{name} must be a 64-bit integer register, found XMM register {register}"
        )),
        Operand::Register(register) => match register_width(register) {
            Some(Width::Bits64) => Ok(()),
            Some(width) => Err(format!(
                "{name} must be 64-bit, found {}-bit register",
                width.bits()
            )),
            None => Ok(()),
        },
        Operand::Dereference { address, width } => {
            match resolve_memory_width(address, *width, strings)?.map(memory_width_bits) {
                Some(Width::Bits64) => Ok(()),
                Some(width) => Err(format!(
                    "{name} must be 64-bit, found {}-bit memory operand",
                    width.bits()
                )),
                None => Err(format!(
                    "{name} memory operand requires an explicit 64-bit width"
                )),
            }
        }
        Operand::Ident(name) if stack_scalar_slot(stack, name).is_some() => {
            match operand_width(operand, strings, label_name, stack)? {
                Some(Width::Bits64) => Ok(()),
                Some(width) => Err(format!(
                    "{name} must be 64-bit, found {}-bit stack variable",
                    width.bits()
                )),
                None => Ok(()),
            }
        }
        Operand::StringProperty { .. } => Ok(()),
        _ => Ok(()),
    }
}

fn emit_operand(
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<String, String> {
    match operand {
        Operand::Converted { .. } | Operand::Cast { .. } => Err(String::from(
            "Conversion operands are only supported as assignment sources",
        )),
        Operand::AddressOf(_) => Err(String::from(
            "Address-of operands are only supported as assignment sources",
        )),
        Operand::Dereference { address, width } => {
            let emitted_address = emit_address(address);

            Ok(match resolve_memory_width(address, *width, strings)? {
                Some(width) => format!("{} ptr [{}]", width.ptr(), emitted_address),
                None => format!("[{emitted_address}]"),
            })
        }
        Operand::FloatLiteral(value) => Err(format!(
            "Float literal {value} requires an explicit floating-point operator width"
        )),
        Operand::Immediate(value) => Ok(value.to_string()),
        Operand::Register(name) => Ok(name.clone()),
        Operand::Ident(name) => match stack_scalar_slot(stack, name) {
            Some((offset, width)) => Ok(format!("{} ptr [rbp - {}]", width.ptr(), offset)),
            None if stack_string_slot(stack, name).is_some() => Err(format!(
                "String stack variable {name:?} in label {label_name:?} cannot be used as an operand"
            )),
            None => match strings
                .integers
                .get(&(label_name.to_string(), name.clone()))
            {
                Some(binding) => Ok(binding.value.to_string()),
                None if strings
                    .float_bindings
                    .contains_key(&(label_name.to_string(), name.clone())) =>
                {
                    Err(format!(
                        "Float binding {name:?} in label {label_name:?} requires a floating-point operator width"
                    ))
                }
                None if strings
                    .bindings
                    .contains_key(&(label_name.to_string(), name.clone())) =>
                {
                    Err(format!(
                        "String binding {name:?} in label {label_name:?} cannot be used as an operand"
                    ))
                }
                None => Err(format!("Unknown binding {name:?} in label {label_name:?}")),
            },
        },
        Operand::StringProperty { name, property } => {
            if let Some(offset) = stack_string_property_slot(stack, name, *property) {
                return Ok(format!("qword ptr [rbp - {offset}]"));
            }

            let binding = strings
                .bindings
                .get(&(label_name.to_string(), name.clone()))
                .ok_or_else(|| {
                    format!("Unknown string binding {name:?} in label {label_name:?}")
                })?;

            Ok(match property {
                StringProperty::Len => binding.value.len().to_string(),
                StringProperty::Ptr => format!("offset {}", binding.asm_label),
            })
        }
        Operand::Pointer(name) => Err(format!(
            "Pointer operand &{name} is only supported as the right side of assignment"
        )),
    }
}

fn emit_address(address: &Address) -> String {
    let mut value = emit_address_term(&address.first);

    for (operator, term) in &address.rest {
        match operator {
            AddressOperator::Add => value.push_str(" + "),
            AddressOperator::Subtract => value.push_str(" - "),
        }

        value.push_str(&emit_address_term(term));
    }

    value
}

fn emit_address_term(term: &AddressTerm) -> String {
    match term {
        AddressTerm::Immediate(value) => value.to_string(),
        AddressTerm::Register(name) => name.clone(),
        AddressTerm::ScaledRegister { register, scale } => format!("{register} * {scale}"),
        AddressTerm::Ident(name) => name.clone(),
    }
}

fn float_move_opcode(
    src: &Operand,
    dst: &Operand,
    strings: &StringTable,
    stack: &StackFrame,
) -> Result<Option<&'static str>, String> {
    match (src, dst) {
        (Operand::Register(src), Operand::Register(dst))
            if is_xmm_register(src) && is_xmm_register(dst) =>
        {
            Ok(Some("movaps"))
        }
        (Operand::Register(register), memory) if is_xmm_register(register) => {
            float_memory_width(memory, strings, stack)?
                .map(float_move_opcode_for_width)
                .transpose()
        }
        (memory, Operand::Register(register)) if is_xmm_register(register) => {
            float_memory_width(memory, strings, stack)?
                .map(float_move_opcode_for_width)
                .transpose()
        }
        _ => Ok(None),
    }
}

fn float_move_opcode_for_width(width: MemoryWidth) -> Result<&'static str, String> {
    match width {
        MemoryWidth::F32 => Ok("movss"),
        MemoryWidth::F64 => Ok("movsd"),
        _ => Err(String::from(
            "XMM moves require an explicitly f32 or f64 memory operand",
        )),
    }
}

fn register_alias(name: &str, width: Width) -> Result<String, String> {
    let family = crate::register::family(name)
        .ok_or_else(|| format!("Expected integer register, found {name}"))?;

    let alias = match (family, width) {
        ("rax", Width::Bits64) => "rax",
        ("rax", Width::Bits32) => "eax",
        ("rax", Width::Bits16) => "ax",
        ("rax", Width::Bits8) => "al",
        ("rbx", Width::Bits64) => "rbx",
        ("rbx", Width::Bits32) => "ebx",
        ("rbx", Width::Bits16) => "bx",
        ("rbx", Width::Bits8) => "bl",
        ("rcx", Width::Bits64) => "rcx",
        ("rcx", Width::Bits32) => "ecx",
        ("rcx", Width::Bits16) => "cx",
        ("rcx", Width::Bits8) => "cl",
        ("rdx", Width::Bits64) => "rdx",
        ("rdx", Width::Bits32) => "edx",
        ("rdx", Width::Bits16) => "dx",
        ("rdx", Width::Bits8) => "dl",
        ("rdi", Width::Bits64) => "rdi",
        ("rdi", Width::Bits32) => "edi",
        ("rdi", Width::Bits16) => "di",
        ("rdi", Width::Bits8) => "dil",
        ("rsi", Width::Bits64) => "rsi",
        ("rsi", Width::Bits32) => "esi",
        ("rsi", Width::Bits16) => "si",
        ("rsi", Width::Bits8) => "sil",
        ("rbp", Width::Bits64) => "rbp",
        ("rbp", Width::Bits32) => "ebp",
        ("rbp", Width::Bits16) => "bp",
        ("rbp", Width::Bits8) => "bpl",
        ("rsp", Width::Bits64) => "rsp",
        ("rsp", Width::Bits32) => "esp",
        ("rsp", Width::Bits16) => "sp",
        ("rsp", Width::Bits8) => "spl",
        ("r8", Width::Bits64) => "r8",
        ("r8", Width::Bits32) => "r8d",
        ("r8", Width::Bits16) => "r8w",
        ("r8", Width::Bits8) => "r8b",
        ("r9", Width::Bits64) => "r9",
        ("r9", Width::Bits32) => "r9d",
        ("r9", Width::Bits16) => "r9w",
        ("r9", Width::Bits8) => "r9b",
        ("r10", Width::Bits64) => "r10",
        ("r10", Width::Bits32) => "r10d",
        ("r10", Width::Bits16) => "r10w",
        ("r10", Width::Bits8) => "r10b",
        ("r11", Width::Bits64) => "r11",
        ("r11", Width::Bits32) => "r11d",
        ("r11", Width::Bits16) => "r11w",
        ("r11", Width::Bits8) => "r11b",
        ("r12", Width::Bits64) => "r12",
        ("r12", Width::Bits32) => "r12d",
        ("r12", Width::Bits16) => "r12w",
        ("r12", Width::Bits8) => "r12b",
        ("r13", Width::Bits64) => "r13",
        ("r13", Width::Bits32) => "r13d",
        ("r13", Width::Bits16) => "r13w",
        ("r13", Width::Bits8) => "r13b",
        ("r14", Width::Bits64) => "r14",
        ("r14", Width::Bits32) => "r14d",
        ("r14", Width::Bits16) => "r14w",
        ("r14", Width::Bits8) => "r14b",
        ("r15", Width::Bits64) => "r15",
        ("r15", Width::Bits32) => "r15d",
        ("r15", Width::Bits16) => "r15w",
        ("r15", Width::Bits8) => "r15b",
        _ => return Err(format!("Expected integer register, found {name}")),
    };

    Ok(alias.to_string())
}

fn operand_uses_high_byte_register(operand: &Operand) -> bool {
    match operand {
        Operand::Converted { operand, .. } | Operand::Cast { operand, .. } => {
            operand_uses_high_byte_register(operand)
        }
        Operand::AddressOf(address) => address_uses_register(address, is_high_byte_register),
        Operand::Register(name) => is_high_byte_register(name),
        Operand::Dereference { address, .. } => {
            address_uses_register(address, is_high_byte_register)
        }
        _ => false,
    }
}

fn operand_uses_extended_register(operand: &Operand) -> bool {
    match operand {
        Operand::Converted { operand, .. } | Operand::Cast { operand, .. } => {
            operand_uses_extended_register(operand)
        }
        Operand::AddressOf(address) => address_uses_register(address, is_extended_register),
        Operand::Register(name) => is_extended_register(name),
        Operand::Dereference { address, .. } => {
            address_uses_register(address, is_extended_register)
        }
        _ => false,
    }
}

fn operand_uses_xmm_register(operand: &Operand) -> bool {
    match operand {
        Operand::Converted { operand, .. } | Operand::Cast { operand, .. } => {
            operand_uses_xmm_register(operand)
        }
        Operand::AddressOf(address) => address_uses_register(address, is_xmm_register),
        Operand::Register(name) => is_xmm_register(name),
        Operand::Dereference { address, .. } => address_uses_register(address, is_xmm_register),
        _ => false,
    }
}

fn operand_uses_register_family(operand: &Operand, register: &str) -> bool {
    match operand {
        Operand::Converted { operand, .. } | Operand::Cast { operand, .. } => {
            operand_uses_register_family(operand, register)
        }
        Operand::AddressOf(address) => address_uses_register_family(address, register),
        Operand::Register(name) => same_register_family(name, register),
        Operand::Dereference { address, .. } => address_uses_register_family(address, register),
        _ => false,
    }
}

fn operand_address_uses_register_family(operand: &Operand, register: &str) -> bool {
    match operand {
        Operand::Converted { operand, .. } | Operand::Cast { operand, .. } => {
            operand_address_uses_register_family(operand, register)
        }
        Operand::AddressOf(address) => address_uses_register_family(address, register),
        Operand::Dereference { address, .. } => address_uses_register_family(address, register),
        _ => false,
    }
}

fn address_uses_register_family(address: &Address, register: &str) -> bool {
    address_term_uses_register_family(&address.first, register)
        || address
            .rest
            .iter()
            .any(|(_, term)| address_term_uses_register_family(term, register))
}

fn address_term_uses_register_family(term: &AddressTerm, register: &str) -> bool {
    match term {
        AddressTerm::Register(name) | AddressTerm::ScaledRegister { register: name, .. } => {
            same_register_family(name, register)
        }
        _ => false,
    }
}

fn same_register_family(left: &str, right: &str) -> bool {
    crate::register::family(left)
        .is_some_and(|family| crate::register::family(right) == Some(family))
}

fn address_uses_register(address: &Address, predicate: fn(&str) -> bool) -> bool {
    address_term_uses_register(&address.first, predicate)
        || address
            .rest
            .iter()
            .any(|(_, term)| address_term_uses_register(term, predicate))
}

fn address_term_uses_register(term: &AddressTerm, predicate: fn(&str) -> bool) -> bool {
    match term {
        AddressTerm::Register(name) | AddressTerm::ScaledRegister { register: name, .. } => {
            predicate(name)
        }
        _ => false,
    }
}

fn is_high_byte_register(name: &str) -> bool {
    matches!(name, "ah" | "bh" | "ch" | "dh")
}

fn is_extended_register(name: &str) -> bool {
    matches!(
        name,
        "r8" | "r9"
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

fn is_xmm_register(name: &str) -> bool {
    crate::register::is_xmm(name)
}
