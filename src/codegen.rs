use crate::ast::{
    Address, AddressOperator, AddressTerm, AssignmentTarget, AssignmentValue, BindingValue,
    CompareOp, Condition, FloatMathOp, Instruction, Label, MathOp, MemoryDeclaration, MemoryWidth,
    Operand, PrintPart, Program, ReadSource, StringInitializer, StringProperty,
};
use std::collections::{HashMap, HashSet, VecDeque};

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
                    validate_pop_operand(dst, &stack)?;
                    let dst = emit_operand(dst, &strings, &label.name, &stack)?;
                    asm.push_str(&format!("  pop {dst}\n"));
                }
                Instruction::Push { src } => {
                    validate_push_operand(src, &strings, &label.name, &stack)?;
                    let src = emit_operand(src, &strings, &label.name, &stack)?;
                    asm.push_str(&format!("  push {src}\n"));
                }
                Instruction::Read { src, dst, len } => {
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
    if operand_uses_xmm_register(lhs)
        || operand_uses_xmm_register(rhs)
        || is_float_memory_operand(lhs)
        || is_float_memory_operand(rhs)
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

    if let (Some(lhs_width), Some(rhs_width)) =
        (operand_width(lhs, stack), operand_width(rhs, stack))
        && lhs_width != rhs_width
    {
        return Err(format!(
            "Cannot compare {}-bit operand with {}-bit operand",
            lhs_width.bits(),
            rhs_width.bits()
        ));
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

fn collect_string_bindings(program: &Program) -> Result<StringTable, String> {
    let mut all = Vec::new();
    let mut bindings = HashMap::new();
    let mut integers = HashMap::new();
    let mut literals = HashMap::new();
    let mut stack_strings = HashMap::new();
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
                        BindingValue::Float { value, width } => {
                            validate_float_width(*width)?;
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
                _ => {}
            }
        }
    }

    Ok(StringTable {
        all,
        bindings,
        literals,
        integers,
        stack_strings,
    })
}

fn validate_float_width(width: MemoryWidth) -> Result<(), String> {
    if matches!(width, MemoryWidth::F32 | MemoryWidth::F64) {
        Ok(())
    } else {
        Err(String::from("Float bindings require f32 or f64 width"))
    }
}

fn build_stack_frame(label: &Label) -> Result<StackFrame, String> {
    let mut slots = HashMap::new();
    let mut offset = 0;

    for instruction in &label.instructions {
        if let Instruction::Stack { name, width, .. } = instruction {
            offset += memory_width_size(width).max(8);
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

    let label_positions: HashMap<&str, usize> = label
        .instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| match instruction {
            Instruction::Label { name } => Some((name.as_str(), index)),
            _ => None,
        })
        .collect();
    let mut pending = VecDeque::from([0]);
    let mut visited = HashSet::new();

    while let Some(index) = pending.pop_front() {
        if !visited.insert(index) {
            continue;
        }

        let instruction = label.instructions.get(index).ok_or_else(|| {
            format!(
                "Label {:?} declares stack variables but can fall through",
                label.name
            )
        })?;

        match instruction {
            Instruction::Ret | Instruction::Exit { .. } => {}
            Instruction::Syscall
                if previous_instructions_set_exit_syscall(&label.instructions, index) => {}
            Instruction::Jmp { target, condition } => {
                let target_index = *label_positions.get(target.as_str()).ok_or_else(|| {
                    format!(
                        "Unknown local jump target {target:?} in label {:?}",
                        label.name
                    )
                })?;
                pending.push_back(target_index);
                if condition.is_some() {
                    pending.push_back(index + 1);
                }
            }
            _ => pending.push_back(index + 1),
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
                | AssignmentValue::FloatBinary { lhs, rhs, .. }
                | AssignmentValue::WideMultiply { lhs, rhs, .. }
                | AssignmentValue::WideDivide { lhs, rhs, .. } => {
                    operands.push(lhs);
                    operands.push(rhs);
                }
            }
        }
        Instruction::Jmp {
            condition: Some(condition),
            ..
        } => {
            operands.push(&condition.lhs);
            operands.push(&condition.rhs);
        }
        Instruction::Jmp {
            condition: None, ..
        } => {}
        Instruction::Print { parts } => {
            for part in parts {
                if let PrintPart::Operand(operand) = part {
                    operands.push(operand);
                }
            }
        }
        Instruction::Pop { dst } => operands.push(dst),
        Instruction::Push { src } => operands.push(src),
        Instruction::Read { dst, len, .. } => {
            operands.push(dst);
            operands.push(len);
        }
        Instruction::Stack { value, .. } => operands.push(value),
        Instruction::StackString { value, .. } => match value {
            StringInitializer::Literal(_) => {}
            StringInitializer::Slice { ptr, len } => {
                operands.push(ptr);
                operands.push(len);
            }
        },
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
        asm.push_str(&format!("  {} {value}\n", memory_width_directive(width)));
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
    if operand_uses_xmm_register(operand) || is_float_memory_operand(operand) {
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

    match operand_width(len, stack) {
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

    match operand_width(len, stack) {
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

            validate_binary_assignment_does_not_clobber_rhs_address(dst, rhs)?;

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
        AssignmentValue::FloatBinary {
            width,
            op,
            lhs,
            rhs,
        } => emit_float_binary_assignment(
            asm, dst, *width, *op, lhs, rhs, strings, label_name, stack,
        ),
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

            validate_wide_math_rhs_not_clobbered("Widened multiply right operand", rhs, false)?;

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

            validate_wide_math_rhs_not_clobbered("Widened division right operand", rhs, true)?;

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

    validate_float_math_operand("Floating-point arithmetic left operand", lhs, width)?;
    validate_float_math_operand("Floating-point arithmetic right operand", rhs, width)?;

    if lhs != dst {
        emit_float_copy_instruction(asm, lhs, dst, width, strings, label_name, stack)?;
    }

    let rhs = emit_operand(rhs, strings, label_name, stack)?;
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
    let src = emit_operand(src, strings, label_name, stack)?;
    let dst = emit_operand(dst, strings, label_name, stack)?;
    asm.push_str(&format!(
        "  {} {dst}, {src}\n",
        float_move_opcode_for_width(width)?
    ));

    Ok(())
}

fn validate_float_math_operand(
    name: &str,
    operand: &Operand,
    width: MemoryWidth,
) -> Result<(), String> {
    match operand {
        Operand::Register(register) if is_xmm_register(register) => Ok(()),
        Operand::Dereference {
            width: Some(memory_width),
            ..
        } if *memory_width == width => Ok(()),
        Operand::Dereference {
            width: Some(MemoryWidth::F32 | MemoryWidth::F64),
            ..
        } => Err(format!(
            "{name} width must match the floating-point operator width"
        )),
        Operand::Dereference { width: None, .. } => Err(format!(
            "{name} memory operand requires an explicit f32 or f64 width"
        )),
        Operand::Dereference { .. } => Err(format!(
            "{name} must be an XMM register or floating-point memory operand"
        )),
        Operand::Immediate(_) => Err(format!(
            "{name} cannot be an immediate value; use a floating-point memory operand for now"
        )),
        Operand::Ident(_) | Operand::StringProperty { .. } => {
            Err(format!("{name} cannot be a const or stack binding for now"))
        }
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

    if let Some(width) = operand_width(operand, stack)
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
    if let Some(opcode) = float_move_opcode(src, dst)? {
        let src = emit_operand(src, strings, label_name, stack)?;
        let dst = emit_operand(dst, strings, label_name, stack)?;
        asm.push_str(&format!("  {opcode} {dst}, {src}\n"));

        Ok(())
    } else if operand_uses_xmm_register(src) || operand_uses_xmm_register(dst) {
        Err(String::from(
            "XMM moves require one XMM register and one explicitly f32 or f64 memory operand",
        ))
    } else if is_float_memory_operand(src) || is_float_memory_operand(dst) {
        Err(String::from(
            "Floating-point memory operands require an XMM register source or destination",
        ))
    } else if let Operand::Pointer(name) = src {
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
    if opcode != "mov"
        && (operand_uses_xmm_register(src)
            || operand_uses_xmm_register(dst)
            || is_float_memory_operand(src)
            || is_float_memory_operand(dst))
    {
        return Err(format!(
            "{opcode} does not support floating-point operands yet"
        ));
    }

    if matches!(
        dst,
        Operand::Immediate(_) | Operand::Pointer(_) | Operand::StringProperty { .. }
    ) || matches!(dst, Operand::Ident(name) if stack_scalar_slot(stack, name).is_none())
    {
        return Err(format!(
            "{opcode} destination must be a register or memory operand"
        ));
    }

    if matches!(src, Operand::Pointer(_)) {
        return Err(format!("{opcode} source cannot be an address-of operand"));
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
        && src_width != dst_width
    {
        return Err(format!(
            "Cannot use {}-bit source with {}-bit destination",
            src_width.bits(),
            dst_width.bits()
        ));
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

fn destination_width(operand: &Operand, stack: &StackFrame) -> Option<ImmediateDestination> {
    match operand {
        Operand::Register(name) => register_width(name).map(ImmediateDestination::Register),
        Operand::Dereference {
            width: Some(width), ..
        } => Some(ImmediateDestination::Memory(*width)),
        Operand::Ident(name) => {
            stack_scalar_slot(stack, name).map(|(_, width)| ImmediateDestination::Memory(width))
        }
        _ => None,
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
            ImmediateDestination::Memory(width) => memory_width_bits(&width).bits(),
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
    if matches!(src, Operand::Pointer(_)) {
        return Err(String::from("push source cannot be an address-of operand"));
    }

    if is_immediate_operand(src, strings, label_name, stack) {
        return Ok(());
    }

    validate_stack_width("push source", src, stack)
}

fn validate_pop_operand(dst: &Operand, stack: &StackFrame) -> Result<(), String> {
    if matches!(
        dst,
        Operand::Immediate(_) | Operand::Pointer(_) | Operand::StringProperty { .. }
    ) {
        return Err(String::from(
            "pop destination must be a 64-bit register or explicitly 64-bit memory operand",
        ));
    }

    validate_stack_width("pop destination", dst, stack)
}

fn validate_stack_width(name: &str, operand: &Operand, stack: &StackFrame) -> Result<(), String> {
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
        Operand::Ident(name) if stack_scalar_slot(stack, name).is_some() => {
            match operand_width(operand, stack) {
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
        Operand::Dereference { address, width } => {
            let address = emit_address(address);

            Ok(match width {
                Some(width) => format!("{} ptr [{}]", memory_width_ptr(width), address),
                None => format!("[{address}]"),
            })
        }
        Operand::Immediate(value) => Ok(value.to_string()),
        Operand::Register(name) => Ok(name.clone()),
        Operand::Ident(name) => match stack_scalar_slot(stack, name) {
            Some((offset, width)) => Ok(format!(
                "{} ptr [rbp - {}]",
                memory_width_ptr(&width),
                offset
            )),
            None if stack_string_slot(stack, name).is_some() => Err(format!(
                "String stack variable {name:?} in label {label_name:?} cannot be used as an operand"
            )),
            None => match strings
                .integers
                .get(&(label_name.to_string(), name.clone()))
            {
                Some(binding) => Ok(binding.value.to_string()),
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

fn operand_width(operand: &Operand, stack: &StackFrame) -> Option<Width> {
    match operand {
        Operand::Register(name) => register_width(name),
        Operand::Dereference { width, .. } => width.as_ref().map(memory_width_bits),
        Operand::Ident(name) => {
            stack_scalar_slot(stack, name).map(|(_, width)| memory_width_bits(&width))
        }
        Operand::StringProperty { .. } => Some(Width::Bits64),
        _ => None,
    }
}

fn float_move_opcode(src: &Operand, dst: &Operand) -> Result<Option<&'static str>, String> {
    match (src, dst) {
        (Operand::Register(register), memory) if is_xmm_register(register) => {
            float_memory_width(memory)
                .map(float_move_opcode_for_width)
                .transpose()
        }
        (memory, Operand::Register(register)) if is_xmm_register(register) => {
            float_memory_width(memory)
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

fn float_memory_width(operand: &Operand) -> Option<MemoryWidth> {
    match operand {
        Operand::Dereference {
            width: Some(MemoryWidth::F32 | MemoryWidth::F64),
            ..
        } => match operand {
            Operand::Dereference {
                width: Some(width), ..
            } => Some(*width),
            _ => unreachable!(),
        },
        _ => None,
    }
}

fn is_float_memory_operand(operand: &Operand) -> bool {
    float_memory_width(operand).is_some()
}

fn memory_width_bits(width: &MemoryWidth) -> Width {
    match width {
        MemoryWidth::F32 => Width::Bits32,
        MemoryWidth::F64 => Width::Bits64,
        MemoryWidth::I8 | MemoryWidth::U8 => Width::Bits8,
        MemoryWidth::I16 | MemoryWidth::U16 => Width::Bits16,
        MemoryWidth::I32 | MemoryWidth::U32 => Width::Bits32,
        MemoryWidth::I64 | MemoryWidth::U64 => Width::Bits64,
    }
}

fn memory_width_size(width: &MemoryWidth) -> usize {
    match width {
        MemoryWidth::F32 => 4,
        MemoryWidth::F64 => 8,
        MemoryWidth::I8 | MemoryWidth::U8 => 1,
        MemoryWidth::I16 | MemoryWidth::U16 => 2,
        MemoryWidth::I32 | MemoryWidth::U32 => 4,
        MemoryWidth::I64 | MemoryWidth::U64 => 8,
    }
}

fn memory_width_directive(width: &MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::F32 => ".float",
        MemoryWidth::F64 => ".double",
        MemoryWidth::I8 | MemoryWidth::U8 => ".byte",
        MemoryWidth::I16 | MemoryWidth::U16 => ".word",
        MemoryWidth::I32 | MemoryWidth::U32 => ".long",
        MemoryWidth::I64 | MemoryWidth::U64 => ".quad",
    }
}

fn memory_width_ptr(width: &MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::F32 => "dword",
        MemoryWidth::F64 => "qword",
        MemoryWidth::I8 | MemoryWidth::U8 => "byte",
        MemoryWidth::I16 | MemoryWidth::U16 => "word",
        MemoryWidth::I32 | MemoryWidth::U32 => "dword",
        MemoryWidth::I64 | MemoryWidth::U64 => "qword",
    }
}

fn register_width(name: &str) -> Option<Width> {
    crate::register::width(name)
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
        Operand::Register(name) => is_high_byte_register(name),
        Operand::Dereference { address, .. } => {
            address_uses_register(address, is_high_byte_register)
        }
        _ => false,
    }
}

fn operand_uses_extended_register(operand: &Operand) -> bool {
    match operand {
        Operand::Register(name) => is_extended_register(name),
        Operand::Dereference { address, .. } => {
            address_uses_register(address, is_extended_register)
        }
        _ => false,
    }
}

fn operand_uses_xmm_register(operand: &Operand) -> bool {
    match operand {
        Operand::Register(name) => is_xmm_register(name),
        Operand::Dereference { address, .. } => address_uses_register(address, is_xmm_register),
        _ => false,
    }
}

fn operand_uses_register_family(operand: &Operand, register: &str) -> bool {
    match operand {
        Operand::Register(name) => same_register_family(name, register),
        Operand::Dereference { address, .. } => address_uses_register_family(address, register),
        _ => false,
    }
}

fn operand_address_uses_register_family(operand: &Operand, register: &str) -> bool {
    match operand {
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
