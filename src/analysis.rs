use crate::ast::{
    Address, AddressTerm, AssignmentTarget, AssignmentValue, BindingValue, CompareOp,
    ConditionExpr, ControlTarget, Instruction, Label, MemoryWidth, Operand, PrintPart,
    RegisterPair, StringInitializer, StringProperty,
};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone)]
pub(crate) struct StringBinding {
    pub(crate) asm_label: String,
    pub(crate) value: String,
}

#[derive(Clone, Copy)]
pub(crate) struct IntegerBinding {
    pub(crate) value: i128,
    pub(crate) width: Option<MemoryWidth>,
}

#[derive(Clone)]
pub(crate) struct FloatBinding {
    pub(crate) asm_label: String,
    pub(crate) value: String,
    pub(crate) width: MemoryWidth,
}

pub(crate) struct StringTable {
    pub(crate) all: Vec<StringBinding>,
    pub(crate) bindings: HashMap<(String, String), StringBinding>,
    pub(crate) float_bindings: HashMap<(String, String), FloatBinding>,
    pub(crate) float_literals: HashMap<(String, MemoryWidth, String), FloatBinding>,
    pub(crate) floats: Vec<FloatBinding>,
    pub(crate) literals: HashMap<(String, usize), StringBinding>,
    pub(crate) memory_widths: HashMap<String, MemoryWidth>,
    pub(crate) integers: HashMap<(String, String), IntegerBinding>,
    pub(crate) stack_strings: HashMap<(String, String), StringBinding>,
}

pub(crate) fn collect_string_bindings(
    program: &crate::ast::Program,
) -> Result<StringTable, String> {
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
            crate::ast::MemoryDeclaration::Scalar { name, width, .. }
            | crate::ast::MemoryDeclaration::FloatScalar { name, width, .. }
            | crate::ast::MemoryDeclaration::Buffer { name, width, .. }
            | crate::ast::MemoryDeclaration::Array { name, width, .. }
            | crate::ast::MemoryDeclaration::Repeat { name, width, .. } => (name.clone(), *width),
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
                        BindingValue::Integer { value, width } => {
                            integers.insert(
                                key.clone(),
                                IntegerBinding {
                                    value: *value,
                                    width: *width,
                                },
                            );
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
                    value: AssignmentValue::IntrinsicCall { width, args, .. },
                    ..
                } if width.is_float() => {
                    for arg in args {
                        collect_float_literal_operand(
                            &mut floats,
                            &mut float_literals,
                            &label.name,
                            *width,
                            arg,
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
        AssignmentValue::IntrinsicCall { width, args, .. } if width.is_float() => {
            for arg in args {
                collect_float_literal_operand(floats, float_literals, label_name, *width, arg)?;
            }
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
        _ => {}
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

pub(crate) fn validate_float_width(width: MemoryWidth) -> Result<(), String> {
    if width.is_float() {
        Ok(())
    } else {
        Err(String::from("Float bindings require f32 or f64 width"))
    }
}

pub(crate) fn validate_float_literal(value: &str, width: MemoryWidth) -> Result<(), String> {
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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Width {
    Bits8,
    Bits16,
    Bits32,
    Bits64,
}

impl Width {
    pub(crate) fn bits(self) -> u8 {
        match self {
            Width::Bits8 => 8,
            Width::Bits16 => 16,
            Width::Bits32 => 32,
            Width::Bits64 => 64,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ImmediateDestination {
    Register(Width),
    Memory(MemoryWidth),
}

impl ImmediateDestination {
    pub(crate) fn bits(self) -> u8 {
        match self {
            ImmediateDestination::Register(width) => width.bits(),
            ImmediateDestination::Memory(width) => memory_width_bits(width).bits(),
        }
    }
}

pub(crate) fn immediate_value(
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Option<i128> {
    match operand {
        Operand::Immediate(value) => Some(*value),
        Operand::Ident(name) if !stack.slots.contains_key(name) => strings
            .integers
            .get(&(label_name.to_string(), name.clone()))
            .map(|binding| binding.value),
        _ => None,
    }
}

pub(crate) fn destination_width(
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

pub(crate) fn operand_width(
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<Option<Width>, String> {
    match operand {
        Operand::Converted { operand, .. } | Operand::Cast { operand, .. } => {
            operand_width(operand, strings, label_name, stack)
        }
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

pub(crate) fn resolve_memory_width(
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

pub(crate) fn memory_width_bits(width: MemoryWidth) -> Width {
    match width {
        MemoryWidth::F32 => Width::Bits32,
        MemoryWidth::F64 => Width::Bits64,
        MemoryWidth::I8 | MemoryWidth::U8 => Width::Bits8,
        MemoryWidth::I16 | MemoryWidth::U16 => Width::Bits16,
        MemoryWidth::I32 | MemoryWidth::U32 => Width::Bits32,
        MemoryWidth::I64 | MemoryWidth::U64 | MemoryWidth::Ptr => Width::Bits64,
    }
}

pub(crate) fn register_width(name: &str) -> Option<Width> {
    crate::register::width(name)
}

pub(crate) fn float_memory_width(
    operand: &Operand,
    strings: &StringTable,
    stack: &StackFrame,
) -> Result<Option<MemoryWidth>, String> {
    match operand {
        Operand::Converted { operand, .. } | Operand::Cast { operand, .. } => {
            float_memory_width(operand, strings, stack)
        }
        Operand::Dereference { address, width } => {
            Ok(resolve_memory_width(address, *width, strings)?.filter(|width| width.is_float()))
        }
        Operand::Ident(name) => Ok(stack_scalar_slot(stack, name)
            .map(|(_, width)| width)
            .filter(|width| width.is_float())),
        _ => Ok(None),
    }
}

pub(crate) fn is_float_memory_operand(
    operand: &Operand,
    strings: &StringTable,
    stack: &StackFrame,
) -> Result<bool, String> {
    Ok(float_memory_width(operand, strings, stack)?.is_some())
}

#[derive(Clone, Copy)]
pub(crate) enum StackSlot {
    Scalar {
        offset: usize,
        width: MemoryWidth,
    },
    String {
        ptr_offset: usize,
        len_offset: usize,
    },
}

pub(crate) struct StackFrame {
    pub(crate) slots: HashMap<String, StackSlot>,
    pub(crate) size: usize,
}

impl StackFrame {
    pub(crate) fn has_slots(&self) -> bool {
        !self.slots.is_empty()
    }
}

impl StackSlot {
    pub(crate) fn scalar(self) -> Option<(usize, MemoryWidth)> {
        match self {
            StackSlot::Scalar { offset, width } => Some((offset, width)),
            StackSlot::String { .. } => None,
        }
    }

    pub(crate) fn string(self) -> Option<(usize, usize)> {
        match self {
            StackSlot::String {
                ptr_offset,
                len_offset,
            } => Some((ptr_offset, len_offset)),
            StackSlot::Scalar { .. } => None,
        }
    }
}

pub(crate) fn stack_scalar_slot(stack: &StackFrame, name: &str) -> Option<(usize, MemoryWidth)> {
    stack.slots.get(name).and_then(|slot| slot.scalar())
}

pub(crate) fn stack_string_slot(stack: &StackFrame, name: &str) -> Option<(usize, usize)> {
    stack.slots.get(name).and_then(|slot| slot.string())
}

pub(crate) fn stack_string_property_slot(
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

pub(crate) fn build_stack_frame_from_layout(
    layout: &crate::ir::StackLayout,
    alignment: usize,
) -> StackFrame {
    let mut slots = HashMap::new();
    let mut offset = 0;

    for slot in &layout.slots {
        if let crate::ir::StackSlot::Scalar { name, width } = slot {
            offset += width.size().max(8);
            slots.insert(
                name.clone(),
                StackSlot::Scalar {
                    offset,
                    width: *width,
                },
            );
        } else if let crate::ir::StackSlot::String { name } = slot {
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

    StackFrame {
        slots,
        size: align_to(offset, alignment),
    }
}

pub(crate) fn validate_label(
    label: &Label,
    top_level_labels: &HashSet<&str>,
    stack: &StackFrame,
    frame_pointer: &str,
) -> Result<(), String> {
    validate_stack_register_use(label, stack, frame_pointer)?;
    validate_label_control_flow(label, top_level_labels)
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
                if let ControlTarget::Label(target) = target
                    && !top_level_labels.contains(target.as_str())
                {
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
                match target {
                    ControlTarget::Label(target) => {
                        if !is_local_label_target(target)
                            || top_level_labels.contains(target.as_str())
                        {
                            return Err(format!(
                                "jmp target {target:?} in function {:?} must be a local label",
                                label.name
                            ));
                        }

                        let target_index =
                            *label_positions.get(target.as_str()).ok_or_else(|| {
                                format!(
                                    "Unknown local jump target {target:?} in label {:?}",
                                    label.name
                                )
                            })?;
                        pending.push_back((target_index, depth));
                    }
                    ControlTarget::Operand(_) => {
                        if depth != 0 {
                            return Err(format!(
                                "Function {:?} cannot indirect jmp with unbalanced manual stack depth {depth}. Pop pushed values before the jump.",
                                label.name
                            ));
                        }
                    }
                }
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

fn validate_stack_register_use(
    label: &Label,
    stack: &StackFrame,
    frame_pointer: &str,
) -> Result<(), String> {
    if !stack.has_slots() {
        return Ok(());
    }

    for instruction in &label.instructions {
        validate_instruction_does_not_use_frame_pointer(instruction, &label.name, frame_pointer)?;
    }

    Ok(())
}

fn validate_instruction_does_not_use_frame_pointer(
    instruction: &Instruction,
    label_name: &str,
    frame_pointer: &str,
) -> Result<(), String> {
    if let Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(RegisterPair { high, low }),
        ..
    } = instruction
        && (high == frame_pointer || low == frame_pointer)
    {
        return Err(format!(
            "Label {label_name:?} declares stack variables, so {frame_pointer} is reserved"
        ));
    }

    if let Instruction::Assign {
        value: AssignmentValue::PairBinary { lhs, rhs, .. },
        ..
    } = instruction
        && (register_pair_uses_frame_pointer(lhs, frame_pointer)
            || register_pair_uses_frame_pointer(rhs, frame_pointer))
    {
        return Err(format!(
            "Label {label_name:?} declares stack variables, so {frame_pointer} is reserved"
        ));
    }

    let mut uses_rbp = false;
    instruction.visit_operands(|operand| {
        uses_rbp |= operand_uses_frame_pointer(operand, frame_pointer);
    });

    if uses_rbp {
        return Err(format!(
            "Label {label_name:?} declares stack variables, so {frame_pointer} is reserved"
        ));
    }

    Ok(())
}

fn register_pair_uses_frame_pointer(pair: &RegisterPair, frame_pointer: &str) -> bool {
    pair.high == frame_pointer || pair.low == frame_pointer
}

fn operand_uses_frame_pointer(operand: &Operand, frame_pointer: &str) -> bool {
    match operand {
        Operand::Register(name) => name == frame_pointer,
        Operand::Dereference { address, .. } => address_uses_frame_pointer(address, frame_pointer),
        _ => false,
    }
}

fn address_uses_frame_pointer(address: &Address, frame_pointer: &str) -> bool {
    address_term_uses_frame_pointer(&address.first, frame_pointer)
        || address
            .rest
            .iter()
            .any(|(_, term)| address_term_uses_frame_pointer(term, frame_pointer))
}

fn address_term_uses_frame_pointer(term: &AddressTerm, frame_pointer: &str) -> bool {
    match term {
        AddressTerm::Register(name) | AddressTerm::ScaledRegister { register: name, .. } => {
            name == frame_pointer
        }
        _ => false,
    }
}

fn is_local_label_target(target: &str) -> bool {
    target.starts_with(".L.")
}

fn align_to(value: usize, alignment: usize) -> usize {
    value.div_ceil(alignment) * alignment
}
