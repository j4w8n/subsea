use crate::ast::{
    Address, AddressOperator, AddressTerm, AssignmentTarget, AssignmentValue, BindingValue,
    CompareOp, Condition, Instruction, Label, MathOp, MemoryDeclaration, MemoryWidth, Operand,
    PrintPart, Program,
};
use std::collections::HashMap;

pub fn emit_x86_64_linux_asm(program: &Program) -> Result<String, String> {
    let strings = collect_string_bindings(program)?;
    let mut literal_indexes = HashMap::new();
    let mut asm = String::new();

    asm.push_str(".intel_syntax noprefix\n");
    emit_data(&mut asm, &program.memory);
    emit_bss(&mut asm, &program.memory);
    emit_rodata(&mut asm, &strings.all);
    asm.push_str(".section .text\n");
    asm.push_str(".global _start\n\n");
    asm.push_str("_start:\n");
    asm.push_str(&format!("  jmp {}\n\n", program.entry));

    for label in &program.labels {
        let stack = build_stack_frame(label)?;
        validate_stack_control_flow(label, &stack)?;
        validate_stack_register_use(label, &stack)?;

        asm.push_str(&format!("{}:\n", label.name));

        if stack.has_slots() {
            emit_frame_prologue(&mut asm, &stack);
            emit_stack_initializers(&mut asm, &label.instructions, &strings, &label.name, &stack)?;
        }

        let mut runtime_print_index = 0;

        for instruction in &label.instructions {
            match instruction {
                Instruction::Assign { dst, value } => {
                    emit_assignment(&mut asm, dst, value, &strings, &label.name, &stack)?;
                }
                Instruction::Call { target } => {
                    asm.push_str(&format!("  call {target}\n"));
                }
                Instruction::Exit { code } => {
                    asm.push_str("  mov rax, 60\n");
                    asm.push_str(&format!("  mov rdi, {code}\n"));
                    asm.push_str("  syscall\n");
                }
                Instruction::Jmp { target, condition } => {
                    if let Some(condition) = condition {
                        emit_conditional_jump(
                            &mut asm,
                            target,
                            condition,
                            &strings,
                            &label.name,
                            &stack,
                        )?;
                    } else {
                        asm.push_str(&format!("  jmp {target}\n"));
                    }
                }
                Instruction::Label { name } => {
                    asm.push_str(&format!("{name}:\n"));
                }
                Instruction::Const { .. } | Instruction::Stack { .. } => {}
                Instruction::Print { parts } => {
                    for part in parts {
                        match part {
                            PrintPart::Binding(name) => {
                                if stack.slots.contains_key(name) {
                                    runtime_print_index += 1;
                                    emit_print_operand_instruction(
                                        &mut asm,
                                        &Operand::Ident(name.clone()),
                                        &strings,
                                        &label.name,
                                        &stack,
                                        runtime_print_index,
                                    )?;
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
                    validate_pop_operand(dst, &stack)?;
                    let dst = emit_operand(dst, &strings, &label.name, &stack)?;
                    asm.push_str(&format!("  pop {dst}\n"));
                }
                Instruction::Push { src } => {
                    validate_push_operand(src, &strings, &label.name, &stack)?;
                    let src = emit_operand(src, &strings, &label.name, &stack)?;
                    asm.push_str(&format!("  push {src}\n"));
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

        asm.push('\n');
    }

    Ok(asm)
}

fn emit_conditional_jump(
    asm: &mut String,
    target: &str,
    condition: &Condition,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let (lhs, rhs, op) = normalize_compare(
        &condition.lhs,
        &condition.rhs,
        condition.op,
        strings,
        label_name,
        stack,
    )?;

    validate_compare_operands(lhs, rhs, strings, label_name, stack)?;

    let lhs = emit_operand(lhs, strings, label_name, stack)?;
    let rhs = emit_operand(rhs, strings, label_name, stack)?;
    asm.push_str(&format!("  cmp {lhs}, {rhs}\n"));
    asm.push_str(&format!("  {} {target}\n", compare_jump_opcode(op)));

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
        CompareOp::SignedLess => CompareOp::SignedGreater,
        CompareOp::SignedLessEqual => CompareOp::SignedGreaterEqual,
        CompareOp::SignedGreater => CompareOp::SignedLess,
        CompareOp::SignedGreaterEqual => CompareOp::SignedLessEqual,
        CompareOp::UnsignedLess => CompareOp::UnsignedGreater,
        CompareOp::UnsignedLessEqual => CompareOp::UnsignedGreaterEqual,
        CompareOp::UnsignedGreater => CompareOp::UnsignedLess,
        CompareOp::UnsignedGreaterEqual => CompareOp::UnsignedLessEqual,
    }
}

fn compare_jump_opcode(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Equal => "je",
        CompareOp::NotEqual => "jne",
        CompareOp::SignedLess => "jl",
        CompareOp::SignedLessEqual => "jle",
        CompareOp::SignedGreater => "jg",
        CompareOp::SignedGreaterEqual => "jge",
        CompareOp::UnsignedLess => "jb",
        CompareOp::UnsignedLessEqual => "jbe",
        CompareOp::UnsignedGreater => "ja",
        CompareOp::UnsignedGreaterEqual => "jae",
    }
}

fn validate_compare_operands(
    lhs: &Operand,
    rhs: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(lhs, Operand::Pointer(_)) || matches!(rhs, Operand::Pointer(_)) {
        return Err(String::from("Comparison cannot use an address-of operand"));
    }

    if is_memory_operand(lhs, stack) && is_memory_operand(rhs, stack) {
        return Err(String::from(
            "Comparison cannot use memory for both operands",
        ));
    }

    if let (Some(lhs_width), Some(rhs_width)) =
        (operand_width(lhs, stack), operand_width(rhs, stack))
    {
        if lhs_width != rhs_width {
            return Err(format!(
                "Cannot compare {}-bit operand with {}-bit operand",
                lhs_width.bits(),
                rhs_width.bits()
            ));
        }
    }

    if let (Some(value), Some(width)) = (
        immediate_value(rhs, strings, label_name, stack),
        destination_width(lhs, stack),
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

#[derive(Clone)]
struct StringBinding {
    asm_label: String,
    value: String,
}

struct StringTable {
    all: Vec<StringBinding>,
    bindings: HashMap<(String, String), StringBinding>,
    literals: HashMap<(String, usize), StringBinding>,
    integers: HashMap<(String, String), IntegerBinding>,
}

struct StackFrame {
    slots: HashMap<String, StackSlot>,
    size: usize,
}

#[derive(Clone, Copy)]
struct StackSlot {
    offset: usize,
    width: MemoryWidth,
}

impl StackFrame {
    fn has_slots(&self) -> bool {
        !self.slots.is_empty()
    }
}

#[derive(Clone, Copy)]
struct IntegerBinding {
    value: i64,
}

fn collect_string_bindings(program: &Program) -> Result<StringTable, String> {
    let mut all = Vec::new();
    let mut bindings = HashMap::new();
    let mut integers = HashMap::new();
    let mut literals = HashMap::new();
    let mut literal_indexes = HashMap::new();

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
                _ => {}
            }
        }
    }

    Ok(StringTable {
        all,
        bindings,
        literals,
        integers,
    })
}

fn build_stack_frame(label: &Label) -> Result<StackFrame, String> {
    let mut slots = HashMap::new();
    let mut offset = 0;

    for instruction in &label.instructions {
        if let Instruction::Stack { name, width, .. } = instruction {
            offset += memory_width_size(width).max(8);
            slots.insert(
                name.clone(),
                StackSlot {
                    offset,
                    width: *width,
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
        if let Instruction::Stack { name, value, .. } = instruction {
            if !is_immediate_operand(value, strings, label_name, stack) {
                return Err(format!(
                    "Stack variable {name:?} initializer must be an integer immediate or const"
                ));
            }

            let dst = Operand::Ident(name.clone());
            emit_copy_instruction(asm, value, &dst, strings, label_name, stack)?;
        }
    }

    Ok(())
}

fn validate_stack_control_flow(label: &Label, stack: &StackFrame) -> Result<(), String> {
    if !stack.has_slots() {
        return Ok(());
    }

    for instruction in &label.instructions {
        match instruction {
            Instruction::Jmp { target, .. } if !is_local_label_target(target) => {
                return Err(format!(
                    "Label {:?} declares stack variables and cannot jump to top-level label {target:?}",
                    label.name
                ));
            }
            _ => {}
        }
    }

    match label.instructions.last() {
        Some(Instruction::Ret | Instruction::Exit { .. } | Instruction::Jmp { .. }) => Ok(()),
        _ => Err(format!(
            "Label {:?} declares stack variables but can fall through; end with ret, exit, or local jmp",
            label.name
        )),
    }
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
    let mut operands = Vec::new();

    match instruction {
        Instruction::Assign { dst, value } => {
            match dst {
                AssignmentTarget::Operand(operand) => operands.push(operand),
                AssignmentTarget::RegisterPair { high, low } => {
                    if is_rbp_register(high) || is_rbp_register(low) {
                        return Err(format!(
                            "Label {label_name:?} declares stack variables, so rbp is reserved"
                        ));
                    }
                }
            }

            match value {
                AssignmentValue::Operand(operand) => operands.push(operand),
                AssignmentValue::Binary { lhs, rhs, .. }
                | AssignmentValue::WideMultiply { lhs, rhs, .. }
                | AssignmentValue::WideDivide { lhs, rhs, .. } => {
                    operands.push(lhs);
                    operands.push(rhs);
                }
            }
        }
        Instruction::Jmp { condition, .. } => {
            if let Some(condition) = condition {
                operands.push(&condition.lhs);
                operands.push(&condition.rhs);
            }
        }
        Instruction::Print { parts } => {
            for part in parts {
                if let PrintPart::Operand(operand) = part {
                    operands.push(operand);
                }
            }
        }
        Instruction::Pop { dst } => operands.push(dst),
        Instruction::Push { src } => operands.push(src),
        Instruction::Stack { value, .. } => operands.push(value),
        _ => {}
    }

    if operands.iter().any(|operand| operand_uses_rbp(operand)) {
        return Err(format!(
            "Label {label_name:?} declares stack variables, so rbp is reserved"
        ));
    }

    Ok(())
}

fn operand_is_stack_slot(operand: &Operand, stack: &StackFrame) -> bool {
    matches!(operand, Operand::Ident(name) if stack.slots.contains_key(name))
}

fn is_memory_operand(operand: &Operand, stack: &StackFrame) -> bool {
    matches!(operand, Operand::Dereference { .. }) || operand_is_stack_slot(operand, stack)
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
            MemoryDeclaration::Scalar { name, width, value } => Some((name, width, value)),
            MemoryDeclaration::Buffer { .. } => None,
        })
        .collect();

    if scalars.is_empty() {
        return;
    }

    asm.push_str(".section .data\n");

    for (name, width, value) in scalars {
        asm.push_str(&format!("{name}:\n"));
        asm.push_str(&format!("  {} {value}\n", memory_width_directive(width)));
    }

    asm.push('\n');
}

fn emit_bss(asm: &mut String, memory: &[MemoryDeclaration]) {
    let buffers: Vec<_> = memory
        .iter()
        .filter_map(|declaration| match declaration {
            MemoryDeclaration::Scalar { .. } => None,
            MemoryDeclaration::Buffer { name, width, count } => Some((name, width, count)),
        })
        .collect();

    if buffers.is_empty() {
        return;
    }

    asm.push_str(".section .bss\n");

    for (name, width, count) in buffers {
        asm.push_str(&format!("{name}:\n"));
        asm.push_str(&format!("  .zero {}\n", memory_width_size(width) * count));
    }

    asm.push('\n');
}

fn emit_rodata(asm: &mut String, strings: &[StringBinding]) {
    if strings.is_empty() {
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
    if matches!(operand, Operand::Pointer(_)) {
        return Err(String::from(
            "print operand cannot be an address-of operand",
        ));
    }

    load_print_operand(asm, operand, strings, label_name, stack)?;

    let loop_label = format!(".L.{label_name}.print_{index}_loop");
    let done_label = format!(".L.{label_name}.print_{index}_done");

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

fn load_print_operand(
    asm: &mut String,
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match operand_width(operand, stack) {
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
        AssignmentValue::Binary { op, lhs, rhs } => {
            let dst = assignment_operand_target(dst)?;

            if !matches!(dst, Operand::Register(_)) && *op == MathOp::Multiply {
                return Err(String::from(
                    "Multiply assignment destination must be a register for now",
                ));
            }

            if lhs == dst {
                let opcode = match op {
                    MathOp::Add => "add",
                    MathOp::Multiply => "imul",
                    MathOp::Subtract => "sub",
                };

                return emit_binary_instruction(asm, opcode, rhs, dst, strings, label_name, stack);
            }

            if rhs == dst {
                match op {
                    MathOp::Add | MathOp::Multiply => {
                        let opcode = match op {
                            MathOp::Add => "add",
                            MathOp::Multiply => "imul",
                            MathOp::Subtract => unreachable!(),
                        };

                        return emit_binary_instruction(
                            asm, opcode, lhs, dst, strings, label_name, stack,
                        );
                    }
                    MathOp::Subtract => {
                        let dst_operand = emit_operand(dst, strings, label_name, stack)?;
                        asm.push_str(&format!("  neg {dst_operand}\n"));

                        return emit_binary_instruction(
                            asm, "add", lhs, dst, strings, label_name, stack,
                        );
                    }
                }
            }

            {
                emit_copy_instruction(asm, lhs, dst, strings, label_name, stack)?;

                let opcode = match op {
                    MathOp::Add => "add",
                    MathOp::Multiply => "imul",
                    MathOp::Subtract => "sub",
                };

                emit_binary_instruction(asm, opcode, rhs, dst, strings, label_name, stack)
            }
        }
        AssignmentValue::WideMultiply { signed, lhs, rhs } => {
            validate_wide_math_target("Widened multiply", dst)?;
            validate_wide_math_operand(
                "Widened multiply left operand",
                lhs,
                strings,
                label_name,
                stack,
            )?;
            validate_wide_math_operand(
                "Widened multiply right operand",
                rhs,
                strings,
                label_name,
                stack,
            )?;

            let rax = Operand::Register(String::from("rax"));
            emit_copy_instruction(asm, lhs, &rax, strings, label_name, stack)?;

            let opcode = if *signed { "imul" } else { "mul" };
            let rhs = emit_operand(rhs, strings, label_name, stack)?;
            asm.push_str(&format!("  {opcode} {rhs}\n"));

            Ok(())
        }
        AssignmentValue::WideDivide { signed, lhs, rhs } => {
            validate_wide_math_target("Widened division", dst)?;
            validate_wide_math_operand(
                "Widened division left operand",
                lhs,
                strings,
                label_name,
                stack,
            )?;
            validate_wide_math_operand(
                "Widened division right operand",
                rhs,
                strings,
                label_name,
                stack,
            )?;

            let rax = Operand::Register(String::from("rax"));
            emit_copy_instruction(asm, lhs, &rax, strings, label_name, stack)?;

            if *signed {
                asm.push_str("  cqo\n");
            } else {
                asm.push_str("  xor rdx, rdx\n");
            }

            let opcode = if *signed { "idiv" } else { "div" };
            let rhs = emit_operand(rhs, strings, label_name, stack)?;
            asm.push_str(&format!("  {opcode} {rhs}\n"));

            Ok(())
        }
    }
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

    if let Some(width) = operand_width(operand, stack) {
        if width != Width::Bits64 {
            return Err(format!(
                "{name} must be 64-bit, found {}-bit operand",
                width.bits()
            ));
        }
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
    if let Operand::Pointer(name) = src {
        validate_address_copy_dst(dst)?;

        let dst = emit_operand(dst, strings, label_name, stack)?;
        asm.push_str(&format!("  lea {dst}, [rip + {name}]\n"));

        Ok(())
    } else {
        emit_binary_instruction(asm, "mov", src, dst, strings, label_name, stack)
    }
}

fn validate_binary_operands(
    opcode: &str,
    src: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(dst, Operand::Immediate(_) | Operand::Pointer(_))
        || matches!(dst, Operand::Ident(name) if !stack.slots.contains_key(name))
    {
        return Err(format!(
            "{opcode} destination must be a register or memory operand"
        ));
    }

    if matches!(src, Operand::Pointer(_)) {
        return Err(format!("{opcode} source cannot be an address-of operand"));
    }

    if is_memory_operand(src, stack) && is_memory_operand(dst, stack) {
        return Err(format!(
            "{opcode} cannot use memory for both source and destination"
        ));
    }

    if opcode == "mov"
        && is_immediate_operand(src, strings, label_name, stack)
        && matches!(
            dst,
            Operand::Dereference {
                address: _,
                width: None
            }
        )
    {
        return Err(String::from(
            "Cannot assign an immediate value directly into memory without an explicit width",
        ));
    }

    if let (Some(src_width), Some(dst_width)) =
        (operand_width(src, stack), operand_width(dst, stack))
    {
        if src_width != dst_width {
            return Err(format!(
                "Cannot use {}-bit source with {}-bit destination",
                src_width.bits(),
                dst_width.bits()
            ));
        }
    }

    if let (Some(value), Some(width)) = (
        immediate_value(src, strings, label_name, stack),
        destination_width(dst, stack),
    ) {
        validate_immediate_range(value, width)?;
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
) -> Option<i64> {
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

fn destination_width(operand: &Operand, stack: &StackFrame) -> Option<Width> {
    match operand {
        Operand::Register(name) => register_width(name),
        Operand::Dereference {
            width: Some(width), ..
        } => Some(memory_width_bits(width)),
        Operand::Ident(name) => stack
            .slots
            .get(name)
            .map(|slot| memory_width_bits(&slot.width)),
        _ => None,
    }
}

fn validate_immediate_range(value: i64, width: Width) -> Result<(), String> {
    let valid = match width {
        Width::Bits8 => i8::MIN as i64 <= value && value <= u8::MAX as i64,
        Width::Bits16 => i16::MIN as i64 <= value && value <= u16::MAX as i64,
        Width::Bits32 => i32::MIN as i64 <= value && value <= u32::MAX as i64,
        Width::Bits64 => true,
    };

    if valid {
        Ok(())
    } else {
        Err(format!(
            "Immediate value {value} does not fit in {}-bit destination",
            width.bits()
        ))
    }
}

fn validate_address_copy_dst(dst: &Operand) -> Result<(), String> {
    match dst {
        Operand::Register(_) => Ok(()),
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
    if matches!(src, Operand::Pointer(_)) {
        return Err(String::from("push source cannot be an address-of operand"));
    }

    if is_immediate_operand(src, strings, label_name, stack) {
        return Ok(());
    }

    validate_stack_width("push source", src, stack)
}

fn validate_pop_operand(dst: &Operand, stack: &StackFrame) -> Result<(), String> {
    if matches!(dst, Operand::Immediate(_) | Operand::Pointer(_)) {
        return Err(String::from(
            "pop destination must be a 64-bit register or explicitly 64-bit memory operand",
        ));
    }

    validate_stack_width("pop destination", dst, stack)
}

fn validate_stack_width(name: &str, operand: &Operand, stack: &StackFrame) -> Result<(), String> {
    match operand {
        Operand::Register(register) => match register_width(register) {
            Some(Width::Bits64) => Ok(()),
            Some(width) => Err(format!(
                "{name} must be 64-bit, found {}-bit register",
                width.bits()
            )),
            None => Ok(()),
        },
        Operand::Dereference { width, .. } => match width.as_ref().map(memory_width_bits) {
            Some(Width::Bits64) => Ok(()),
            Some(width) => Err(format!(
                "{name} must be 64-bit, found {}-bit memory operand",
                width.bits()
            )),
            None => Err(format!(
                "{name} memory operand requires an explicit 64-bit width"
            )),
        },
        Operand::Ident(name) if stack.slots.contains_key(name) => {
            match operand_width(operand, stack) {
                Some(Width::Bits64) => Ok(()),
                Some(width) => Err(format!(
                    "{name} must be 64-bit, found {}-bit stack variable",
                    width.bits()
                )),
                None => Ok(()),
            }
        }
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
        Operand::Dereference { address, width } => {
            let address = emit_address(address);

            Ok(match width {
                Some(width) => format!("{} ptr [{}]", memory_width_ptr(width), address),
                None => format!("[{address}]"),
            })
        }
        Operand::Immediate(value) => Ok(value.to_string()),
        Operand::Register(name) => Ok(name.clone()),
        Operand::Ident(name) => match stack.slots.get(name) {
            Some(slot) => Ok(format!(
                "{} ptr [rbp - {}]",
                memory_width_ptr(&slot.width),
                slot.offset
            )),
            None => match strings
                .integers
                .get(&(label_name.to_string(), name.clone()))
            {
                Some(binding) => Ok(binding.value.to_string()),
                None => Ok(name.clone()),
            },
        },
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

fn operand_width(operand: &Operand, stack: &StackFrame) -> Option<Width> {
    match operand {
        Operand::Register(name) => register_width(name),
        Operand::Dereference { width, .. } => width.as_ref().map(memory_width_bits),
        Operand::Ident(name) => stack
            .slots
            .get(name)
            .map(|slot| memory_width_bits(&slot.width)),
        _ => None,
    }
}

fn memory_width_bits(width: &MemoryWidth) -> Width {
    match width {
        MemoryWidth::I8 | MemoryWidth::U8 => Width::Bits8,
        MemoryWidth::I16 | MemoryWidth::U16 => Width::Bits16,
        MemoryWidth::I32 | MemoryWidth::U32 => Width::Bits32,
        MemoryWidth::I64 | MemoryWidth::U64 => Width::Bits64,
    }
}

fn memory_width_size(width: &MemoryWidth) -> usize {
    match width {
        MemoryWidth::I8 | MemoryWidth::U8 => 1,
        MemoryWidth::I16 | MemoryWidth::U16 => 2,
        MemoryWidth::I32 | MemoryWidth::U32 => 4,
        MemoryWidth::I64 | MemoryWidth::U64 => 8,
    }
}

fn memory_width_directive(width: &MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::I8 | MemoryWidth::U8 => ".byte",
        MemoryWidth::I16 | MemoryWidth::U16 => ".word",
        MemoryWidth::I32 | MemoryWidth::U32 => ".long",
        MemoryWidth::I64 | MemoryWidth::U64 => ".quad",
    }
}

fn memory_width_ptr(width: &MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::I8 | MemoryWidth::U8 => "byte",
        MemoryWidth::I16 | MemoryWidth::U16 => "word",
        MemoryWidth::I32 | MemoryWidth::U32 => "dword",
        MemoryWidth::I64 | MemoryWidth::U64 => "qword",
    }
}

fn register_width(name: &str) -> Option<Width> {
    match name {
        "rax" | "rbx" | "rcx" | "rdx" | "rdi" | "rsi" | "rbp" | "rsp" | "r8" | "r9" | "r10"
        | "r11" | "r12" | "r13" | "r14" | "r15" => Some(Width::Bits64),
        "eax" | "ebx" | "ecx" | "edx" | "edi" | "esi" | "ebp" | "esp" | "r8d" | "r9d" | "r10d"
        | "r11d" | "r12d" | "r13d" | "r14d" | "r15d" => Some(Width::Bits32),
        "ax" | "bx" | "cx" | "dx" | "di" | "si" | "bp" | "sp" | "r8w" | "r9w" | "r10w" | "r11w"
        | "r12w" | "r13w" | "r14w" | "r15w" => Some(Width::Bits16),
        "al" | "bl" | "cl" | "dl" | "ah" | "bh" | "ch" | "dh" | "dil" | "sil" | "bpl" | "spl"
        | "r8b" | "r9b" | "r10b" | "r11b" | "r12b" | "r13b" | "r14b" | "r15b" => Some(Width::Bits8),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Width {
    Bits8,
    Bits16,
    Bits32,
    Bits64,
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
