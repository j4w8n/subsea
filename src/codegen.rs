use crate::ast::{
    Address, AddressOperator, AddressTerm, AssignmentTarget, AssignmentValue, BindingValue,
    BitwiseUnaryOp, CompareOp, Condition, ConditionExpr, FloatMathOp, Instruction, Label, MathOp,
    MemoryDeclaration, MemoryWidth, Operand, PrintPart, Program, ReadSource, StringInitializer,
    StringProperty, WidthConversion,
};
use std::collections::{HashMap, HashSet, VecDeque};

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

pub fn emit_x86_64_asm(program: &Program, target: Target) -> Result<String, String> {
    emit_x86_64_asm_with_entry_symbol(program, target, "_start")
}

pub fn emit_x86_64_asm_with_entry_symbol(
    program: &Program,
    target: Target,
    entry_symbol: &str,
) -> Result<String, String> {
    let strings = collect_string_bindings(program)?;
    let mut literal_indexes = HashMap::new();
    let mut asm = String::new();
    let labels = LabelSymbols {
        source_entry: &program.entry,
        entry_symbol,
    };

    asm.push_str(".intel_syntax noprefix\n");
    emit_data(&mut asm, &program.memory);
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
        let stack = build_stack_frame(label)?;
        validate_stack_register_use(label, &stack)?;

        asm.push_str(&format!("{}:\n", labels.emit_label(&label.name)));

        if stack.has_slots() {
            emit_frame_prologue(&mut asm, &stack);
            emit_stack_initializers(&mut asm, &label.instructions, &strings, &label.name, &stack)?;
        }

        let mut runtime_print_index = 0;
        let mut conditional_jump_index = 0;

        for instruction in &label.instructions {
            match instruction {
                Instruction::Assign { dst, value } => {
                    emit_assignment(&mut asm, dst, value, &strings, &label.name, &stack)?;
                }
                Instruction::AssignIf {
                    dst,
                    value,
                    condition,
                } => {
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
                    asm.push_str(&format!("  call {}\n", labels.emit_label(target)));
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
                Instruction::Halt => {
                    asm.push_str("  hlt\n");
                }
                Instruction::Jmp { target, condition } => {
                    if let Some(condition) = condition {
                        conditional_jump_index += 1;
                        emit_conditional_jump(
                            &mut asm,
                            &labels.emit_label(target),
                            condition,
                            &strings,
                            &label.name,
                            &stack,
                            conditional_jump_index,
                        )?;
                    } else {
                        asm.push_str(&format!("  jmp {}\n", labels.emit_label(target)));
                    }
                }
                Instruction::Label { name } => {
                    asm.push_str(&format!("{name}:\n"));
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
                                        StackSlot::Scalar { .. } => {
                                            emit_print_operand_instruction(
                                                &mut asm,
                                                &Operand::Ident(name.clone()),
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

                    emit_read_instruction(&mut asm, src, dst, len, &strings, &label.name, &stack)?;
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
        }

        validate_label_control_flow(label, &top_level_labels)?;

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

    let lhs = emit_operand(lhs, strings, label_name, stack)?;
    let rhs = emit_operand(rhs, strings, label_name, stack)?;
    let op = if jump_if_true {
        op
    } else {
        invert_compare_op(op)
    };
    asm.push_str(&format!("  cmp {lhs}, {rhs}\n"));
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
        MathOp::BitAnd
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

#[derive(Clone)]
struct StringBinding {
    asm_label: String,
    value: String,
}

struct StringTable {
    all: Vec<StringBinding>,
    bindings: HashMap<(String, String), StringBinding>,
    float_bindings: HashMap<(String, String), FloatBinding>,
    float_literals: HashMap<(String, MemoryWidth, String), FloatBinding>,
    floats: Vec<FloatBinding>,
    literals: HashMap<(String, usize), StringBinding>,
    memory_widths: HashMap<String, MemoryWidth>,
    integers: HashMap<(String, String), IntegerBinding>,
    stack_strings: HashMap<(String, String), StringBinding>,
}

struct StackFrame {
    slots: HashMap<String, StackSlot>,
    size: usize,
}

#[derive(Clone, Copy)]
enum StackSlot {
    Scalar {
        offset: usize,
        width: MemoryWidth,
    },
    String {
        ptr_offset: usize,
        len_offset: usize,
    },
}

impl StackFrame {
    fn has_slots(&self) -> bool {
        !self.slots.is_empty()
    }
}

impl StackSlot {
    fn scalar(self) -> Option<(usize, MemoryWidth)> {
        match self {
            StackSlot::Scalar { offset, width } => Some((offset, width)),
            StackSlot::String { .. } => None,
        }
    }

    fn string(self) -> Option<(usize, usize)> {
        match self {
            StackSlot::String {
                ptr_offset,
                len_offset,
            } => Some((ptr_offset, len_offset)),
            StackSlot::Scalar { .. } => None,
        }
    }
}

fn stack_scalar_slot(stack: &StackFrame, name: &str) -> Option<(usize, MemoryWidth)> {
    stack.slots.get(name).and_then(|slot| slot.scalar())
}

fn stack_string_slot(stack: &StackFrame, name: &str) -> Option<(usize, usize)> {
    stack.slots.get(name).and_then(|slot| slot.string())
}

fn stack_string_property_slot(
    stack: &StackFrame,
    name: &str,
    property: StringProperty,
) -> Option<usize> {
    let (ptr_offset, len_offset) = stack_string_slot(stack, name)?;

    Some(match property {
        StringProperty::Len => len_offset,
        StringProperty::Ptr => ptr_offset,
    })
}

#[derive(Clone, Copy)]
struct IntegerBinding {
    value: i128,
}

#[derive(Clone)]
struct FloatBinding {
    asm_label: String,
    value: String,
    width: MemoryWidth,
}

fn collect_string_bindings(program: &Program) -> Result<StringTable, String> {
    let mut all = Vec::new();
    let mut bindings = HashMap::new();
    let mut float_bindings = HashMap::new();
    let mut float_literals = HashMap::new();
    let mut floats = Vec::new();
    let mut integers = HashMap::new();
    let mut literals = HashMap::new();
    let mut stack_strings = HashMap::new();
    let mut literal_indexes = HashMap::new();
    let memory_widths = program
        .memory
        .iter()
        .map(|declaration| match declaration {
            MemoryDeclaration::Scalar { name, width, .. }
            | MemoryDeclaration::FloatScalar { name, width, .. }
            | MemoryDeclaration::Buffer { name, width, .. } => (name.clone(), *width),
        })
        .collect();

    for label in &program.labels {
        for instruction in &label.instructions {
            match instruction {
                Instruction::Const { name, value } => {
                    let key = (label.name.clone(), name.clone());

                    if bindings.contains_key(&key) {
                        return Err(format!(
                            "Binding {name:?} is already defined in label {:?}",
                            label.name
                        ));
                    }

                    let (asm_label, printable_value) = match value {
                        BindingValue::String(value) => {
                            (format!(".Lstr_{}_{}", label.name, name), value.clone())
                        }
                        BindingValue::Integer { value, .. } => {
                            integers.insert(key.clone(), IntegerBinding { value: *value });
                            (format!(".Lint_{}_{}", label.name, name), value.to_string())
                        }
                        BindingValue::Float { value, width } => {
                            validate_float_width(*width)?;
                            let float = FloatBinding {
                                asm_label: format!(".Lfloatval_{}_{}", label.name, name),
                                value: value.clone(),
                                width: *width,
                            };
                            floats.push(float.clone());
                            float_bindings.insert(key.clone(), float);
                            (format!(".Lfloat_{}_{}", label.name, name), value.clone())
                        }
                    };

                    let binding = StringBinding {
                        asm_label,
                        value: printable_value,
                    };

                    all.push(binding.clone());
                    bindings.insert(key, binding);
                }
                Instruction::Print { parts } => {
                    for part in parts {
                        if let PrintPart::Literal(value) = part {
                            let index = literal_indexes.entry(label.name.clone()).or_insert(0);
                            *index += 1;

                            let binding = StringBinding {
                                asm_label: format!(".Lstr_{}_literal_{}", label.name, index),
                                value: value.clone(),
                            };

                            all.push(binding.clone());
                            literals.insert((label.name.clone(), *index), binding);
                        }
                    }
                }
                Instruction::StackString {
                    name,
                    value: StringInitializer::Literal(value),
                } => {
                    let binding = StringBinding {
                        asm_label: format!(".Lstr_{}_{}", label.name, name),
                        value: value.clone(),
                    };

                    all.push(binding.clone());
                    stack_strings.insert((label.name.clone(), name.clone()), binding);
                }
                Instruction::AssignIf {
                    value, condition, ..
                } => {
                    collect_assignment_value_float_literals(
                        &mut floats,
                        &mut float_literals,
                        &label.name,
                        value,
                    )?;
                    collect_condition_float_literals(
                        &mut floats,
                        &mut float_literals,
                        &label.name,
                        condition,
                    )?;
                }
                Instruction::Assign {
                    value:
                        AssignmentValue::FloatBinary {
                            width, lhs, rhs, ..
                        },
                    ..
                } => {
                    collect_float_literal_operand(
                        &mut floats,
                        &mut float_literals,
                        &label.name,
                        *width,
                        lhs,
                    )?;
                    collect_float_literal_operand(
                        &mut floats,
                        &mut float_literals,
                        &label.name,
                        *width,
                        rhs,
                    )?;
                }
                Instruction::Assign {
                    value: AssignmentValue::Binary { lhs, rhs, .. },
                    ..
                } => {
                    for width in [MemoryWidth::F32, MemoryWidth::F64] {
                        collect_float_literal_operand(
                            &mut floats,
                            &mut float_literals,
                            &label.name,
                            width,
                            lhs,
                        )?;
                        collect_float_literal_operand(
                            &mut floats,
                            &mut float_literals,
                            &label.name,
                            width,
                            rhs,
                        )?;
                    }
                }
                Instruction::Assign {
                    value: AssignmentValue::Condition(condition),
                    ..
                }
                | Instruction::Jmp {
                    condition: Some(condition),
                    ..
                } => collect_condition_float_literals(
                    &mut floats,
                    &mut float_literals,
                    &label.name,
                    condition,
                )?,
                Instruction::Stack { width, value, .. } if width.is_float() => {
                    collect_float_literal_operand(
                        &mut floats,
                        &mut float_literals,
                        &label.name,
                        *width,
                        value,
                    )?;
                }
                _ => {}
            }
        }
    }

    Ok(StringTable {
        all,
        bindings,
        float_bindings,
        float_literals,
        floats,
        literals,
        memory_widths,
        integers,
        stack_strings,
    })
}

fn collect_assignment_value_float_literals(
    floats: &mut Vec<FloatBinding>,
    float_literals: &mut HashMap<(String, MemoryWidth, String), FloatBinding>,
    label_name: &str,
    value: &AssignmentValue,
) -> Result<(), String> {
    match value {
        AssignmentValue::FloatBinary {
            width, lhs, rhs, ..
        } => {
            collect_float_literal_operand(floats, float_literals, label_name, *width, lhs)?;
            collect_float_literal_operand(floats, float_literals, label_name, *width, rhs)?;
        }
        AssignmentValue::Binary { lhs, rhs, .. } => {
            for width in [MemoryWidth::F32, MemoryWidth::F64] {
                collect_float_literal_operand(floats, float_literals, label_name, width, lhs)?;
                collect_float_literal_operand(floats, float_literals, label_name, width, rhs)?;
            }
        }
        AssignmentValue::Condition(condition) => {
            collect_condition_float_literals(floats, float_literals, label_name, condition)?;
        }
        AssignmentValue::Operand(_)
        | AssignmentValue::BitwiseUnary { .. }
        | AssignmentValue::WideMultiply { .. }
        | AssignmentValue::WideDivide { .. } => {}
    }

    Ok(())
}

fn collect_condition_float_literals(
    floats: &mut Vec<FloatBinding>,
    float_literals: &mut HashMap<(String, MemoryWidth, String), FloatBinding>,
    label_name: &str,
    condition: &ConditionExpr,
) -> Result<(), String> {
    let ConditionExpr::Compare(condition) = condition else {
        return Ok(());
    };

    if let Some(width) = float_compare_width(condition.op) {
        collect_float_literal_operand(floats, float_literals, label_name, width, &condition.lhs)?;
        collect_float_literal_operand(floats, float_literals, label_name, width, &condition.rhs)?;
    } else if matches!(
        condition.op,
        CompareOp::Equal
            | CompareOp::NotEqual
            | CompareOp::Less
            | CompareOp::LessEqual
            | CompareOp::Greater
            | CompareOp::GreaterEqual
    ) {
        for width in [MemoryWidth::F32, MemoryWidth::F64] {
            collect_float_literal_operand(
                floats,
                float_literals,
                label_name,
                width,
                &condition.lhs,
            )?;
            collect_float_literal_operand(
                floats,
                float_literals,
                label_name,
                width,
                &condition.rhs,
            )?;
        }
    }

    Ok(())
}

fn collect_float_literal_operand(
    floats: &mut Vec<FloatBinding>,
    float_literals: &mut HashMap<(String, MemoryWidth, String), FloatBinding>,
    label_name: &str,
    width: MemoryWidth,
    operand: &Operand,
) -> Result<(), String> {
    let Operand::FloatLiteral(value) = operand else {
        return Ok(());
    };

    validate_float_literal(value, width)?;

    let key = (label_name.to_string(), width, value.clone());
    if float_literals.contains_key(&key) {
        return Ok(());
    }

    let binding = FloatBinding {
        asm_label: format!(".Lfloatlit_{}_{}", label_name, floats.len() + 1),
        value: value.clone(),
        width,
    };

    floats.push(binding.clone());
    float_literals.insert(key, binding);

    Ok(())
}

fn validate_float_width(width: MemoryWidth) -> Result<(), String> {
    if width.is_float() {
        Ok(())
    } else {
        Err(String::from("Float bindings require f32 or f64 width"))
    }
}

fn validate_float_literal(value: &str, width: MemoryWidth) -> Result<(), String> {
    let valid = match width {
        MemoryWidth::F32 => value.parse::<f32>().is_ok_and(f32::is_finite),
        MemoryWidth::F64 => value.parse::<f64>().is_ok_and(f64::is_finite),
        _ => return Err(String::from("Float literal requires f32 or f64 width")),
    };

    if valid {
        Ok(())
    } else {
        Err(format!("Invalid float literal {value:?}"))
    }
}

fn build_stack_frame(label: &Label) -> Result<StackFrame, String> {
    let mut slots = HashMap::new();
    let mut offset = 0;

    for instruction in &label.instructions {
        if let Instruction::Stack { name, width, .. } = instruction {
            offset += width.size().max(8);
            slots.insert(
                name.clone(),
                StackSlot::Scalar {
                    offset,
                    width: *width,
                },
            );
        } else if let Instruction::StackString { name, .. } = instruction {
            offset += 8;
            let ptr_offset = offset;
            offset += 8;
            let len_offset = offset;
            slots.insert(
                name.clone(),
                StackSlot::String {
                    ptr_offset,
                    len_offset,
                },
            );
        }
    }

    Ok(StackFrame {
        slots,
        size: align_to(offset, 16),
    })
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

fn validate_label_control_flow(
    label: &Label,
    top_level_labels: &HashSet<&str>,
) -> Result<(), String> {
    let label_positions: HashMap<&str, usize> = label
        .instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| match instruction {
            Instruction::Label { name } => Some((name.as_str(), index)),
            _ => None,
        })
        .collect();
    let mut pending = VecDeque::from([(0, 0isize)]);
    let mut visited = HashSet::new();
    let mut instruction_depths = HashMap::new();

    while let Some((index, depth)) = pending.pop_front() {
        if depth < 0 {
            return Err(format!(
                "Function {:?} pops more values than it pushes",
                label.name
            ));
        }

        if let Some(previous_depth) = instruction_depths.insert(index, depth)
            && previous_depth != depth
        {
            return Err(format!(
                "Function {:?} reaches instruction {index} with conflicting stack depths {previous_depth} and {depth}",
                label.name
            ));
        }

        if !visited.insert((index, depth)) {
            continue;
        }

        let Some(instruction) = label.instructions.get(index) else {
            if depth != 0 {
                return Err(format!(
                    "Function {:?} can fall through with unbalanced manual stack depth {depth}. Pop pushed values before the function ends, or use `exit` if this path terminates the process.",
                    label.name
                ));
            }

            return Err(format!(
                "Function {:?} can fall through. End this path with `ret`, `exit`, or an unconditional local `jmp` to code that does.",
                label.name
            ));
        };

        match instruction {
            Instruction::Call { target } => {
                if !top_level_labels.contains(target.as_str()) {
                    return Err(format!(
                        "call target {target:?} in function {:?} must be a top-level function",
                        label.name
                    ));
                }
                pending.push_back((index + 1, depth));
            }
            Instruction::Ret => {
                if depth != 0 {
                    return Err(format!(
                        "Function {:?} cannot ret with unbalanced manual stack depth {depth}. Pop pushed values before the function ends, or use `exit` if this path terminates the process.",
                        label.name
                    ));
                }
            }
            Instruction::Exit { .. } => {}
            Instruction::Syscall
                if previous_instructions_set_exit_syscall(&label.instructions, index) => {}
            Instruction::Jmp { target, condition } => {
                if !is_local_label_target(target) || top_level_labels.contains(target.as_str()) {
                    return Err(format!(
                        "jmp target {target:?} in function {:?} must be a local label",
                        label.name
                    ));
                }

                let target_index = *label_positions.get(target.as_str()).ok_or_else(|| {
                    format!(
                        "Unknown local jump target {target:?} in label {:?}",
                        label.name
                    )
                })?;
                pending.push_back((target_index, depth));
                if condition.is_some() {
                    pending.push_back((index + 1, depth));
                }
            }
            Instruction::Pop { .. } => pending.push_back((index + 1, depth - 1)),
            Instruction::Push { .. } => pending.push_back((index + 1, depth + 1)),
            _ => pending.push_back((index + 1, depth)),
        }
    }

    Ok(())
}

fn previous_instructions_set_exit_syscall(
    instructions: &[Instruction],
    syscall_index: usize,
) -> bool {
    let mut rax_is_exit = false;
    let mut rdi_is_set = false;

    for instruction in &instructions[..syscall_index] {
        if let Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Register(register)),
            value,
        } = instruction
        {
            match register.as_str() {
                "rax" => {
                    rax_is_exit = matches!(value, AssignmentValue::Operand(Operand::Immediate(60)));
                }
                "rdi" => rdi_is_set = true,
                _ => {}
            }
        }
    }

    rax_is_exit && rdi_is_set
}

fn validate_stack_register_use(label: &Label, stack: &StackFrame) -> Result<(), String> {
    if !stack.has_slots() {
        return Ok(());
    }

    for instruction in &label.instructions {
        validate_instruction_does_not_use_rbp(instruction, &label.name)?;
    }

    Ok(())
}

fn validate_instruction_does_not_use_rbp(
    instruction: &Instruction,
    label_name: &str,
) -> Result<(), String> {
    if let Instruction::Assign {
        dst: AssignmentTarget::RegisterPair { high, low },
        ..
    } = instruction
        && (is_rbp_register(high) || is_rbp_register(low))
    {
        return Err(format!(
            "Label {label_name:?} declares stack variables, so rbp is reserved"
        ));
    }

    if instruction.operands().into_iter().any(operand_uses_rbp) {
        return Err(format!(
            "Label {label_name:?} declares stack variables, so rbp is reserved"
        ));
    }

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

fn is_local_label_target(target: &str) -> bool {
    target.starts_with(".L.")
}

fn align_to(value: usize, alignment: usize) -> usize {
    value.div_ceil(alignment) * alignment
}

fn is_rbp_register(name: &str) -> bool {
    matches!(name, "rbp" | "ebp" | "bp" | "bpl")
}

fn operand_uses_rbp(operand: &Operand) -> bool {
    match operand {
        Operand::Register(name) => is_rbp_register(name),
        Operand::Dereference { address, .. } => address_uses_rbp(address),
        _ => false,
    }
}

fn address_uses_rbp(address: &Address) -> bool {
    address_term_uses_rbp(&address.first)
        || address
            .rest
            .iter()
            .any(|(_, term)| address_term_uses_rbp(term))
}

fn address_term_uses_rbp(term: &AddressTerm) -> bool {
    match term {
        AddressTerm::Register(name) | AddressTerm::ScaledRegister { register: name, .. } => {
            is_rbp_register(name)
        }
        _ => false,
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
        PrintPart::Operand(_) => Err(String::from("Internal error: operand print is runtime")),
    }
}

fn emit_data(asm: &mut String, memory: &[MemoryDeclaration]) {
    let scalars: Vec<_> = memory
        .iter()
        .filter_map(|declaration| match declaration {
            MemoryDeclaration::Scalar { name, width, value } => {
                Some((name, width, value.to_string()))
            }
            MemoryDeclaration::FloatScalar { name, width, value } => {
                Some((name, width, value.clone()))
            }
            MemoryDeclaration::Buffer { .. } => None,
        })
        .collect();

    if scalars.is_empty() {
        return;
    }

    asm.push_str(".section .data\n");

    for (name, width, value) in scalars {
        asm.push_str(&format!("{name}:\n"));
        asm.push_str(&format!("  {} {value}\n", width.directive()));
    }

    asm.push('\n');
}

fn emit_bss(asm: &mut String, memory: &[MemoryDeclaration]) {
    let buffers: Vec<_> = memory
        .iter()
        .filter_map(|declaration| match declaration {
            MemoryDeclaration::Scalar { .. } | MemoryDeclaration::FloatScalar { .. } => None,
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
    asm.push_str("  mov rax, 1\n");
    asm.push_str("  mov rdi, 1\n");
    asm.push_str(&format!("  lea rsi, [rip + {}]\n", string.asm_label));
    asm.push_str(&format!("  mov rdx, {}\n", string.value.len()));
    asm.push_str("  syscall\n");
}

fn emit_print_operand_instruction(
    asm: &mut String,
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    index: usize,
) -> Result<(), String> {
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

    load_print_operand(asm, operand, strings, label_name, stack)?;

    let loop_label = format!(".L.__subsea.{label_name}.print_{index}_loop");
    let done_label = format!(".L.__subsea.{label_name}.print_{index}_done");

    asm.push_str("  push rbx\n");
    asm.push_str("  sub rsp, 40\n");
    asm.push_str("  lea rsi, [rsp + 40]\n");
    asm.push_str("  mov rbx, 10\n");
    asm.push_str(&format!("{loop_label}:\n"));
    asm.push_str("  xor rdx, rdx\n");
    asm.push_str("  div rbx\n");
    asm.push_str("  add dl, 48\n");
    asm.push_str("  sub rsi, 1\n");
    asm.push_str("  mov byte ptr [rsi], dl\n");
    asm.push_str("  cmp rax, 0\n");
    asm.push_str(&format!("  jne {loop_label}\n"));
    asm.push_str(&format!("{done_label}:\n"));
    asm.push_str("  lea rdx, [rsp + 40]\n");
    asm.push_str("  sub rdx, rsi\n");
    asm.push_str("  mov rax, 1\n");
    asm.push_str("  mov rdi, 1\n");
    asm.push_str("  syscall\n");
    asm.push_str("  add rsp, 40\n");
    asm.push_str("  pop rbx\n");

    Ok(())
}

fn emit_print_stack_string_instruction(
    asm: &mut String,
    name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let (ptr_offset, len_offset) = stack_string_slot(stack, name)
        .ok_or_else(|| format!("Unknown string stack variable {name:?}"))?;

    asm.push_str("  mov rax, 1\n");
    asm.push_str("  mov rdi, 1\n");
    asm.push_str(&format!("  mov rsi, qword ptr [rbp - {ptr_offset}]\n"));
    asm.push_str(&format!("  mov rdx, qword ptr [rbp - {len_offset}]\n"));
    asm.push_str("  syscall\n");

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
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match operand_width(operand, strings, label_name, stack)? {
        Some(Width::Bits8 | Width::Bits16) => {
            let operand = emit_operand(operand, strings, label_name, stack)?;
            asm.push_str(&format!("  movzx rax, {operand}\n"));
        }
        Some(Width::Bits32) => {
            let operand = emit_operand(operand, strings, label_name, stack)?;
            asm.push_str(&format!("  mov eax, {operand}\n"));
        }
        _ => {
            let operand = emit_operand(operand, strings, label_name, stack)?;
            asm.push_str(&format!("  mov rax, {operand}\n"));
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
        AssignmentValue::Binary { op, lhs, rhs } => {
            let dst = assignment_operand_target(dst)?;

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
        AssignmentValue::WideMultiply { signed, lhs, rhs } => emit_wide_math_assignment(
            asm, dst, *signed, false, lhs, rhs, strings, label_name, stack,
        ),
        AssignmentValue::WideDivide { signed, lhs, rhs } => emit_wide_math_assignment(
            asm, dst, *signed, true, lhs, rhs, strings, label_name, stack,
        ),
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

            let lhs = emit_operand(lhs, strings, label_name, stack)?;
            let rhs = emit_operand(rhs, strings, label_name, stack)?;
            asm.push_str(&format!("  cmp {lhs}, {rhs}\n"));

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

    let rax = Operand::Register(String::from("rax"));
    emit_copy_instruction(asm, lhs, &rax, strings, label_name, stack)?;
    validate_wide_math_rhs_not_clobbered(&format!("{prefix} right operand"), rhs, division)?;

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
    let rhs = emit_operand(rhs, strings, label_name, stack)?;
    asm.push_str(&format!("  {opcode} {rhs}\n"));

    Ok(())
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
        Operand::Converted { .. } => Err(format!(
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
        AssignmentTarget::RegisterPair { .. } => Err(String::from(
            "Register-pair assignment requires a widened multiply right side",
        )),
    }
}

fn validate_wide_math_target(operation: &str, dst: &AssignmentTarget) -> Result<(), String> {
    match dst {
        AssignmentTarget::RegisterPair { high, low } if high == "rdx" && low == "rax" => Ok(()),
        AssignmentTarget::RegisterPair { high, low } => Err(format!(
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

    if is_immediate_operand(operand, strings, label_name, stack) {
        return Err(format!("{name} cannot be an immediate value"));
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

fn validate_wide_math_rhs_not_clobbered(
    name: &str,
    operand: &Operand,
    clobbers_rdx_before_rhs: bool,
) -> Result<(), String> {
    if operand_uses_register_family(operand, "rax") {
        return Err(format!(
            "{name} cannot use rax because rax is overwritten before the operation"
        ));
    }

    if clobbers_rdx_before_rhs && operand_uses_register_family(operand, "rdx") {
        return Err(format!(
            "{name} cannot use rdx because rdx is overwritten before the operation"
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
    if let Operand::Converted {
        operand,
        conversion,
    } = src
    {
        emit_width_conversion_copy(asm, operand, *conversion, dst, strings, label_name, stack)
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

    if matches!(src, Operand::Converted { .. }) {
        return Err(format!("{opcode} source cannot use width conversion here"));
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

fn immediate_value(
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Option<i128> {
    match operand {
        Operand::Immediate(value) => Some(*value),
        Operand::Ident(name) => {
            if stack.slots.contains_key(name) {
                None
            } else {
                strings
                    .integers
                    .get(&(label_name.to_string(), name.clone()))
                    .map(|binding| binding.value)
            }
        }
        _ => None,
    }
}

fn destination_width(
    operand: &Operand,
    strings: &StringTable,
    stack: &StackFrame,
) -> Result<Option<ImmediateDestination>, String> {
    match operand {
        Operand::Register(name) => Ok(register_width(name).map(ImmediateDestination::Register)),
        Operand::Dereference { address, width } => {
            Ok(resolve_memory_width(address, *width, strings)?.map(ImmediateDestination::Memory))
        }
        Operand::Ident(name) => Ok(
            stack_scalar_slot(stack, name).map(|(_, width)| ImmediateDestination::Memory(width))
        ),
        _ => Ok(None),
    }
}

#[derive(Clone, Copy)]
enum ImmediateDestination {
    Register(Width),
    Memory(MemoryWidth),
}

impl ImmediateDestination {
    fn bits(self) -> u8 {
        match self {
            ImmediateDestination::Register(width) => width.bits(),
            ImmediateDestination::Memory(width) => memory_width_bits(width).bits(),
        }
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
        ImmediateDestination::Memory(MemoryWidth::I64 | MemoryWidth::U64) => {
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
        Operand::Converted { .. } => Err(String::from(
            "Width conversion operands are only supported as assignment sources",
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
            let offset = stack_string_property_slot(stack, name, *property).ok_or_else(|| {
                format!("Unknown string stack variable {name:?} in label {label_name:?}")
            })?;

            Ok(format!("qword ptr [rbp - {offset}]"))
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

fn operand_width(
    operand: &Operand,
    strings: &StringTable,
    _label_name: &str,
    stack: &StackFrame,
) -> Result<Option<Width>, String> {
    match operand {
        Operand::Converted { operand, .. } => operand_width(operand, strings, _label_name, stack),
        Operand::AddressOf(_) => Ok(Some(Width::Bits64)),
        Operand::Register(name) => Ok(register_width(name)),
        Operand::Dereference { address, width } => {
            Ok(resolve_memory_width(address, *width, strings)?.map(memory_width_bits))
        }
        Operand::Ident(name) => {
            Ok(stack_scalar_slot(stack, name).map(|(_, width)| memory_width_bits(width)))
        }
        Operand::StringProperty { .. } => Ok(Some(Width::Bits64)),
        _ => Ok(None),
    }
}

fn float_move_opcode(
    src: &Operand,
    dst: &Operand,
    strings: &StringTable,
    stack: &StackFrame,
) -> Result<Option<&'static str>, String> {
    match (src, dst) {
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

fn float_memory_width(
    operand: &Operand,
    strings: &StringTable,
    stack: &StackFrame,
) -> Result<Option<MemoryWidth>, String> {
    match operand {
        Operand::Converted { operand, .. } => float_memory_width(operand, strings, stack),
        Operand::AddressOf(_) => Ok(None),
        Operand::Dereference { address, width } => {
            Ok(resolve_memory_width(address, *width, strings)?.filter(|width| width.is_float()))
        }
        Operand::Ident(name) => Ok(stack_scalar_slot(stack, name)
            .map(|(_, width)| width)
            .filter(|width| width.is_float())),
        _ => Ok(None),
    }
}

fn is_float_memory_operand(
    operand: &Operand,
    strings: &StringTable,
    stack: &StackFrame,
) -> Result<bool, String> {
    Ok(float_memory_width(operand, strings, stack)?.is_some())
}

fn resolve_memory_width(
    address: &Address,
    explicit_width: Option<MemoryWidth>,
    strings: &StringTable,
) -> Result<Option<MemoryWidth>, String> {
    if explicit_width.is_some() {
        return Ok(explicit_width);
    }

    let mut inferred = None;
    for term in std::iter::once(&address.first).chain(address.rest.iter().map(|(_, term)| term)) {
        if let AddressTerm::Ident(name) = term
            && let Some(width) = strings.memory_widths.get(name)
        {
            match inferred {
                None => inferred = Some((name.as_str(), *width)),
                Some((existing_name, existing_width))
                    if existing_name == name.as_str() || existing_width == *width => {}
                Some((existing_name, _)) => {
                    return Err(format!(
                        "Memory operand has multiple typed bases {existing_name:?} and {name:?}; add an explicit width"
                    ));
                }
            }
        }
    }

    Ok(inferred.map(|(_, width)| width))
}

fn memory_width_bits(width: MemoryWidth) -> Width {
    match width {
        MemoryWidth::F32 => Width::Bits32,
        MemoryWidth::F64 => Width::Bits64,
        MemoryWidth::I8 | MemoryWidth::U8 => Width::Bits8,
        MemoryWidth::I16 | MemoryWidth::U16 => Width::Bits16,
        MemoryWidth::I32 | MemoryWidth::U32 => Width::Bits32,
        MemoryWidth::I64 | MemoryWidth::U64 => Width::Bits64,
    }
}

fn register_width(name: &str) -> Option<Width> {
    crate::register::width(name)
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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Width {
    Bits8,
    Bits16,
    Bits32,
    Bits64,
}

fn operand_uses_high_byte_register(operand: &Operand) -> bool {
    match operand {
        Operand::Converted { operand, .. } => operand_uses_high_byte_register(operand),
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
        Operand::Converted { operand, .. } => operand_uses_extended_register(operand),
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
        Operand::Converted { operand, .. } => operand_uses_xmm_register(operand),
        Operand::AddressOf(address) => address_uses_register(address, is_xmm_register),
        Operand::Register(name) => is_xmm_register(name),
        Operand::Dereference { address, .. } => address_uses_register(address, is_xmm_register),
        _ => false,
    }
}

fn operand_uses_register_family(operand: &Operand, register: &str) -> bool {
    match operand {
        Operand::Converted { operand, .. } => operand_uses_register_family(operand, register),
        Operand::AddressOf(address) => address_uses_register_family(address, register),
        Operand::Register(name) => same_register_family(name, register),
        Operand::Dereference { address, .. } => address_uses_register_family(address, register),
        _ => false,
    }
}

fn operand_address_uses_register_family(operand: &Operand, register: &str) -> bool {
    match operand {
        Operand::Converted { operand, .. } => {
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

impl Width {
    fn bits(&self) -> u8 {
        match self {
            Width::Bits8 => 8,
            Width::Bits16 => 16,
            Width::Bits32 => 32,
            Width::Bits64 => 64,
        }
    }
}
