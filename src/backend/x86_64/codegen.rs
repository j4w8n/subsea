use super::asm;
use super::{
    family as register_family, is_extended as is_extended_register,
    is_high_byte as is_high_byte_register, is_xmm as is_xmm_register, width as register_width,
};
use crate::analysis::{
    FloatBinding, ImmediateDestination, StackFrame, StackSlot, StringBinding, StringTable, Width,
    build_stack_frame_from_layout, collect_ir_string_bindings, memory_width_bits,
    stack_buffer_slot, stack_scalar_slot, stack_string_slot, validate_float_literal,
    validate_float_width, validate_label,
};
use crate::ast::{
    BitwiseUnaryOp, CompareOp, ExprOp, FloatMathOp, IntrinsicOp, MathOp, MemoryWidth, PairBinaryOp,
    Program,
};
use crate::backend::{
    Architecture, BackendError, RuntimeEmitter, RuntimeOperation, Target, TargetSpec,
};
use crate::diagnostic::ProgramOrigins;
use crate::ir;
use crate::lower;
use crate::platform::linux;
use std::collections::{HashMap, HashSet};

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

struct X86RuntimeEmitter<'a> {
    strings: &'a StringTable,
    literal_indexes: &'a mut HashMap<String, usize>,
    label_name: &'a str,
    stack: &'a StackFrame,
    runtime_print_index: &'a mut usize,
}

impl RuntimeEmitter for X86RuntimeEmitter<'_> {
    fn emit_runtime(
        &mut self,
        asm: &mut String,
        operation: &ir::RuntimeOperation,
    ) -> Result<(), BackendError> {
        match operation {
            ir::RuntimeOperation::Print { parts } => {
                for ir_part in parts {
                    match ir_part {
                        ir::PrintPart::Binding(name) => {
                            if let Some(slot) = self.stack.slots.get(name) {
                                *self.runtime_print_index += 1;
                                match slot {
                                    StackSlot::Scalar { width, .. } => {
                                        let format = infer_ir_print_format_for_width(*width);
                                        emit_print_operand_instruction(
                                            asm,
                                            &ir::Operand::Name(name.clone()),
                                            format,
                                            self.strings,
                                            self.label_name,
                                            self.stack,
                                            *self.runtime_print_index,
                                        )?;
                                    }
                                    StackSlot::String { .. } => {
                                        emit_print_stack_string_instruction(asm, name, self.stack)?;
                                    }
                                    StackSlot::Buffer { .. } => {
                                        return Err(BackendError::new(format!(
                                            "Stack byte buffer {name:?} cannot be printed as a string"
                                        )));
                                    }
                                }
                            } else {
                                let string = resolve_ir_print_part(
                                    self.strings,
                                    self.literal_indexes,
                                    self.label_name,
                                    ir_part,
                                )?;
                                emit_print_string_instruction(asm, string);
                            }
                        }
                        ir::PrintPart::Literal(_) => {
                            let string = resolve_ir_print_part(
                                self.strings,
                                self.literal_indexes,
                                self.label_name,
                                ir_part,
                            )?;
                            emit_print_string_instruction(asm, string);
                        }
                        ir::PrintPart::Operand(operand) => {
                            *self.runtime_print_index += 1;
                            emit_print_operand_instruction(
                                asm,
                                operand,
                                ir::PrintFormat::SignedDecimal(MemoryWidth::I64),
                                self.strings,
                                self.label_name,
                                self.stack,
                                *self.runtime_print_index,
                            )?;
                        }
                        ir::PrintPart::FormattedOperand { format, operand } => {
                            *self.runtime_print_index += 1;
                            emit_print_operand_instruction(
                                asm,
                                operand,
                                *format,
                                self.strings,
                                self.label_name,
                                self.stack,
                                *self.runtime_print_index,
                            )?;
                        }
                    }
                }
                Ok(())
            }
            ir::RuntimeOperation::Read { source, dst, len } => emit_read_instruction(
                asm,
                source,
                dst,
                len,
                self.strings,
                self.label_name,
                self.stack,
            ),
            ir::RuntimeOperation::Release { ptr, len } => {
                emit_release_instruction(asm, ptr, len, self.strings, self.label_name, self.stack)
            }
        }
    }

    fn emit_exit(&mut self, asm: &mut String, code: u8) -> Result<(), BackendError> {
        asm::mov(
            asm,
            asm::Operand::Register(String::from("rdi")),
            asm::Operand::Immediate(code.into()),
        );
        emit_linux_syscall(asm, linux::SYS_EXIT);
        Ok(())
    }

    fn emit_reserve(
        &mut self,
        asm: &mut String,
        dst: &ir::Operand,
        len: &ir::Operand,
    ) -> Result<(), BackendError> {
        emit_ir_linux_memory_size_arg(
            asm,
            "rsi",
            "reserve size",
            len,
            self.strings,
            self.label_name,
            self.stack,
        )
        .map_err(BackendError::new)?;
        asm::mov(
            asm,
            asm::Operand::Register(String::from("rdi")),
            asm::Operand::Immediate(linux::STDIN as i128),
        );
        asm::mov(
            asm,
            asm::Operand::Register(String::from("rdx")),
            asm::Operand::Immediate(3),
        );
        asm::mov(
            asm,
            asm::Operand::Register(String::from("r10")),
            asm::Operand::Immediate(34),
        );
        asm::mov(
            asm,
            asm::Operand::Register(String::from("r8")),
            asm::Operand::Immediate(-1),
        );
        asm::mov(
            asm,
            asm::Operand::Register(String::from("r9")),
            asm::Operand::Immediate(0),
        );
        emit_linux_mmap(asm);
        emit_ir_copy_instruction(
            asm,
            &ir::Operand::TargetRegister(String::from("rax")),
            dst,
            self.strings,
            self.label_name,
            self.stack,
        )
        .map_err(BackendError::new)
    }
}

pub(crate) fn emit_x86_64_asm(program: &Program, target: Target) -> Result<String, String> {
    emit_x86_64_asm_with_entry_symbol(program, target, "_start")
}

pub(crate) fn emit_x86_64_asm_with_entry_symbol(
    program: &Program,
    target: Target,
    entry_symbol: &str,
) -> Result<String, String> {
    emit_x86_64_asm_impl(program, target, entry_symbol, None).map_err(|error| error.message)
}

pub(crate) fn emit_x86_64_asm_with_origins(
    program: &Program,
    target: Target,
    entry_symbol: &str,
    origins: &ProgramOrigins,
) -> Result<String, BackendError> {
    let semantic_ir = lower::lower_program(program)
        .map_err(|error| BackendError::new(error.message).at(error.label, error.instruction))?;
    emit_ir_x86_64_asm_with_origins(&semantic_ir, target, entry_symbol, origins)
}

#[cfg(test)]
pub(crate) fn emit_ir_x86_64_asm(
    program: &ir::Program,
    target: Target,
    entry_symbol: &str,
) -> Result<String, BackendError> {
    emit_ir_x86_64_asm_impl(program, target, entry_symbol, None)
}

fn emit_x86_64_asm_impl(
    program: &Program,
    target: Target,
    entry_symbol: &str,
    origins: Option<&ProgramOrigins>,
) -> Result<String, BackendError> {
    let semantic_ir = lower::lower_program(program)
        .map_err(|error| BackendError::new(error.message).at(error.label, error.instruction))?;
    let assembly = emit_ir_x86_64_asm_impl(&semantic_ir, target, entry_symbol, origins)?;
    validate_x86_ast_labels(program, target)?;
    Ok(assembly)
}

fn validate_x86_ast_labels(program: &Program, target: Target) -> Result<(), BackendError> {
    let top_level_labels: HashSet<&str> = program
        .labels
        .iter()
        .map(|label| label.name.as_str())
        .collect();

    for label in &program.labels {
        let stack = build_stack_frame_from_layout(
            &lower::lower_stack_layout(label),
            target.spec().stack_alignment,
        );
        validate_label(
            label,
            &top_level_labels,
            &stack,
            target.spec().frame_pointer,
            target.spec().exit_syscall,
        )
        .map_err(BackendError::new)?;
    }

    Ok(())
}

pub(crate) fn emit_ir_x86_64_asm_with_origins(
    program: &ir::Program,
    target: Target,
    entry_symbol: &str,
    origins: &ProgramOrigins,
) -> Result<String, BackendError> {
    emit_ir_x86_64_asm_impl(program, target, entry_symbol, Some(origins))
}

fn emit_ir_x86_64_asm_impl(
    program: &ir::Program,
    target: Target,
    entry_symbol: &str,
    origins: Option<&ProgramOrigins>,
) -> Result<String, BackendError> {
    if target.spec().architecture != Architecture::X86_64 {
        return Err(BackendError::new(format!(
            "x86-64 backend cannot compile target {} yet",
            target.name()
        )));
    }

    let strings = collect_ir_string_bindings(program)?;
    let mut literal_indexes = HashMap::new();
    let mut asm = String::new();
    let labels = LabelSymbols {
        source_entry: &program.entry,
        entry_symbol,
    };

    asm::intel_syntax(&mut asm);
    emit_static_data(&mut asm, &program.data, &labels);
    emit_data(&mut asm, &program.memory, &labels);
    emit_bss(&mut asm, &program.memory);
    emit_rodata(&mut asm, &strings.all, &strings.floats);
    asm::text(&mut asm);
    asm::global(&mut asm, entry_symbol);
    asm.push('\n');

    for label in &program.labels {
        let stack = build_stack_frame_from_layout(&label.stack, target.spec().stack_alignment);

        asm::label(&mut asm, labels.emit_label(&label.name));

        if stack.has_slots() {
            emit_frame_prologue(&mut asm, &stack, target.spec());
            emit_ir_stack_buffer_initializers(&mut asm, &stack);
            emit_ir_stack_initializers(
                &mut asm,
                &label.instructions,
                &strings,
                &label.name,
                &stack,
            )?;
        }

        let mut runtime_print_index = 0;
        let mut conditional_jump_index = 0;

        for (instruction_index, instruction) in label.instructions.iter().enumerate() {
            let result: Result<(), BackendError> = (|| {
                match instruction {
                    ir::Instruction::Assign { value, .. } => {
                        if ir_value_uses_linux_reserve(value)
                            && !target.supports_runtime(RuntimeOperation::Reserve)
                        {
                            return Err(BackendError::new(
                                "reserve is only supported for target x86_64",
                            ));
                        }

                        emit_ir_assignment(&mut asm, instruction, &strings, &label.name, &stack)?;
                    }
                    ir::Instruction::AssignIf {
                        dst,
                        value,
                        condition,
                    } => {
                        if ir_value_uses_linux_reserve(value)
                            && !target.supports_runtime(RuntimeOperation::Reserve)
                        {
                            return Err(BackendError::new(
                                "reserve is only supported for target x86_64",
                            ));
                        }

                        conditional_jump_index += 1;
                        let skip_label = format!(
                            ".L.__subsea.{}.assign_if_{}_skip",
                            label.name, conditional_jump_index
                        );
                        emit_ir_condition_jump(
                            &mut asm,
                            &skip_label,
                            condition,
                            false,
                            &strings,
                            &label.name,
                            &stack,
                            conditional_jump_index,
                        )?;
                        emit_ir_assignment(
                            &mut asm,
                            &ir::Instruction::Assign {
                                dst: dst.clone(),
                                value: value.clone(),
                            },
                            &strings,
                            &label.name,
                            &stack,
                        )?;
                        asm::label(&mut asm, skip_label);
                    }
                    ir::Instruction::Call { target } => {
                        emit_ir_call_instruction(
                            &mut asm,
                            target,
                            &labels,
                            &strings,
                            &label.name,
                            &stack,
                        )?;
                    }
                    ir::Instruction::Exit { code } => {
                        if !target.supports_runtime(RuntimeOperation::Exit) {
                            return Err(BackendError::new(
                                "exit is only supported for target x86; use asm.x86 \"hlt\" or an explicit loop for x86-free",
                            ));
                        }

                        let mut emitter = X86RuntimeEmitter {
                            strings: &strings,
                            literal_indexes: &mut literal_indexes,
                            label_name: &label.name,
                            stack: &stack,
                            runtime_print_index: &mut runtime_print_index,
                        };
                        emitter.emit_exit(&mut asm, *code)?;
                    }
                    ir::Instruction::InlineAsm { text, .. } => {
                        asm::instruction(&mut asm, text);
                    }
                    ir::Instruction::Jmp { target, condition } => {
                        conditional_jump_index += usize::from(condition.is_some());
                        emit_ir_jmp_instruction(
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
                    ir::Instruction::Label { name } => {
                        asm::label(&mut asm, name.clone());
                    }
                    ir::Instruction::Nop => {
                        asm::nop(&mut asm);
                    }
                    ir::Instruction::Const { .. }
                    | ir::Instruction::Stack { .. }
                    | ir::Instruction::StackBuffer { .. } => {}
                    ir::Instruction::StackString { name, value } => {
                        emit_ir_stack_string_initializer(
                            &mut asm,
                            name,
                            value,
                            &strings,
                            &label.name,
                            &stack,
                        )?;
                    }
                    ir::Instruction::Runtime(operation) => {
                        if !target.supports_runtime(RuntimeOperation::Write) {
                            return Err(BackendError::new(
                                "print is only supported for target x86_64",
                            ));
                        }

                        let mut emitter = X86RuntimeEmitter {
                            strings: &strings,
                            literal_indexes: &mut literal_indexes,
                            label_name: &label.name,
                            stack: &stack,
                            runtime_print_index: &mut runtime_print_index,
                        };
                        emitter.emit_runtime(&mut asm, operation)?;
                    }
                    ir::Instruction::Pop { dst } => {
                        validate_ir_pop_operand(dst, &strings, &stack)?;
                        let dst = emit_ir_operand(dst, &strings, &label.name, &stack)?;
                        asm::pop(&mut asm, asm::Operand::Address(dst));
                    }
                    ir::Instruction::Push { src } => {
                        validate_ir_push_operand(src, &strings, &label.name, &stack)?;
                        let src = emit_ir_operand(src, &strings, &label.name, &stack)?;
                        asm::push(&mut asm, asm::Operand::Address(src));
                    }
                    ir::Instruction::PairAssign { dst, op, lhs, rhs } => {
                        emit_ir_pair_assignment(&mut asm, dst, *op, lhs, rhs)?;
                    }
                    ir::Instruction::WideAssign {
                        dst,
                        signed,
                        division,
                        lhs,
                        rhs,
                    } => {
                        emit_ir_wide_math_assignment(
                            &mut asm,
                            dst,
                            *signed,
                            *division,
                            lhs,
                            rhs,
                            &strings,
                            &label.name,
                            &stack,
                        )?;
                    }
                    ir::Instruction::Ret => {
                        emit_ir_return_instruction(&mut asm, instruction, &stack, target.spec());
                    }
                    ir::Instruction::Syscall => {
                        asm::syscall_trap(&mut asm);
                    }
                }
                Ok(())
            })();
            if let Err(error) = result {
                return Err(if origins.is_some() && error.instruction.is_none() {
                    error.at(&label.name, instruction_index)
                } else {
                    error
                });
            }
        }

        asm.push('\n');
    }

    Ok(asm)
}

fn emit_ir_stack_buffer_initializers(asm: &mut String, stack: &StackFrame) {
    for slot in stack.slots.values() {
        if let StackSlot::Buffer { offset, count } = slot {
            asm::push(asm, asm::Operand::Register(String::from("rdi")));
            asm::push(asm, asm::Operand::Register(String::from("rcx")));
            asm::instruction(asm, "xor eax, eax");
            asm::lea(
                asm,
                asm::Operand::Register(String::from("rdi")),
                format!("[rbp - {offset}]"),
            );
            asm::mov(
                asm,
                asm::Operand::Register(String::from("rcx")),
                asm::Operand::Immediate(*count as i128),
            );
            asm::instruction(asm, "rep stosb");
            asm::pop(asm, asm::Operand::Register(String::from("rcx")));
            asm::pop(asm, asm::Operand::Register(String::from("rdi")));
        }
    }
}

fn emit_ir_stack_initializers(
    asm: &mut String,
    instructions: &[ir::Instruction],
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    for instruction in instructions {
        if let ir::Instruction::Stack { name, width, value } = instruction {
            if width.is_float() {
                emit_ir_stack_float_initializer(
                    asm, name, *width, value, strings, label_name, stack,
                )?;
            } else {
                emit_ir_copy_instruction(
                    asm,
                    value,
                    &ir::Operand::Name(name.clone()),
                    strings,
                    label_name,
                    stack,
                )?;
            }
        }
    }
    Ok(())
}

fn emit_ir_stack_string_initializer(
    asm: &mut String,
    name: &str,
    value: &ir::StringInitializer,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let (ptr_offset, len_offset) = stack_string_slot(stack, name)
        .ok_or_else(|| format!("Unknown string stack variable {name:?}"))?;

    match value {
        ir::StringInitializer::Literal(_) => {
            let string = strings
                .stack_strings
                .get(&(label_name.to_string(), name.to_string()))
                .ok_or_else(|| format!("Unknown string literal for stack variable {name:?}"))?;
            emit_stack_string_address(asm, &string.asm_label, ptr_offset);
            asm::mov(
                asm,
                asm::Operand::Address(format!("qword ptr [rbp - {len_offset}]")),
                asm::Operand::Immediate(string.value.len() as i128),
            );
        }
        ir::StringInitializer::Slice { ptr, len } => {
            emit_ir_stack_string_slice_pointer(asm, ptr, strings, label_name, stack, ptr_offset)?;
            emit_ir_stack_string_slice_len(asm, len, strings, label_name, stack, len_offset)?;
        }
    }
    Ok(())
}

fn emit_ir_stack_float_initializer(
    asm: &mut String,
    name: &str,
    width: MemoryWidth,
    value: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let (offset, _) =
        stack_scalar_slot(stack, name).ok_or_else(|| format!("Unknown stack variable {name:?}"))?;
    validate_ir_float_initializer(value, width, strings, label_name, stack)?;
    let src = emit_ir_float_operand(value, width, strings, label_name, stack)?;

    asm::push(asm, asm::Operand::Register(String::from("rax")));
    match width {
        MemoryWidth::F32 => {
            asm::instruction(asm, format_args!("mov eax, {src}"));
            asm::instruction(asm, format_args!("mov dword ptr [rbp - {offset}], eax"));
        }
        MemoryWidth::F64 => {
            asm::instruction(asm, format_args!("mov rax, {src}"));
            asm::instruction(asm, format_args!("mov qword ptr [rbp - {offset}], rax"));
        }
        _ => unreachable!(),
    }
    asm::pop(asm, asm::Operand::Register(String::from("rax")));
    Ok(())
}

fn validate_ir_float_initializer(
    value: &ir::Operand,
    width: MemoryWidth,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match value {
        ir::Operand::TargetRegister(register) if is_xmm_register(register) => Ok(()),
        ir::Operand::FloatLiteral(value) => validate_float_literal(value, width),
        ir::Operand::Name(binding) => {
            if let Some((_, stack_width)) = stack_scalar_slot(stack, binding) {
                if stack_width == width && stack_width.is_float() {
                    return Ok(());
                }
                return Err(String::from(
                    "Floating-point stack initializer width must match the floating-point value",
                ));
            }
            match strings
                .float_bindings
                .get(&(label_name.to_string(), binding.clone()))
            {
                Some(float) if float.width == width => Ok(()),
                Some(_) => Err(String::from(
                    "Floating-point stack initializer width must match the floating-point value",
                )),
                None => Err(String::from(
                    "Floating-point stack initializer must use a float binding, literal, or XMM register",
                )),
            }
        }
        ir::Operand::Memory {
            address,
            width: value_width,
        } => {
            let value_width = value_width.or_else(|| match &address.first {
                ir::AddressTerm::Name(name) => strings.memory_widths.get(name).copied(),
                _ => None,
            });
            if value_width == Some(width) {
                Ok(())
            } else {
                Err(String::from(
                    "Floating-point stack initializer width must match the floating-point value",
                ))
            }
        }
        _ => Err(String::from(
            "Floating-point stack initializer must use a float binding, literal, or XMM register",
        )),
    }
}

fn ir_value_uses_linux_reserve(value: &ir::Value) -> bool {
    matches!(value, ir::Value::PlatformReserve { .. })
}

fn emit_ir_jmp_instruction(
    asm: &mut String,
    target: &ir::ControlTarget,
    condition: Option<&ir::Condition>,
    labels: &LabelSymbols,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    index: usize,
) -> Result<(), String> {
    if condition.is_none() {
        match target {
            ir::ControlTarget::Label(target) => {
                asm::jump(asm, asm::Operand::Address(labels.emit_label(target)));
            }
            ir::ControlTarget::Operand(operand) => {
                validate_ir_indirect_control_target("jmp", operand, strings, label_name, stack)?;
                let operand = emit_ir_operand(operand, strings, label_name, stack)?;
                asm::jump(asm, asm::Operand::Address(operand));
            }
        }
        return Ok(());
    }

    match target {
        ir::ControlTarget::Label(target) => emit_ir_condition_jump(
            asm,
            &labels.emit_label(target),
            condition.unwrap(),
            true,
            strings,
            label_name,
            stack,
            index,
        ),
        ir::ControlTarget::Operand(operand) => {
            validate_ir_indirect_control_target("jmp", operand, strings, label_name, stack)?;
            let operand = emit_ir_operand(operand, strings, label_name, stack)?;
            let skip_label = format!(".L.__subsea.{label_name}.indirect_jmp_{index}_skip");
            emit_ir_condition_jump(
                asm,
                &skip_label,
                condition.unwrap(),
                false,
                strings,
                label_name,
                stack,
                index,
            )?;
            asm::jump(asm, asm::Operand::Address(operand));
            asm::label(asm, skip_label);
            Ok(())
        }
    }
}

fn emit_ir_pair_assignment(
    asm: &mut String,
    dst: &ir::RegisterPair,
    op: PairBinaryOp,
    lhs: &ir::RegisterPair,
    rhs: &ir::RegisterPair,
) -> Result<(), String> {
    validate_pair_binary_assignment_ir(dst, lhs, rhs)?;
    let (low, high) = pair_math_opcodes(op);
    asm::instruction(asm, format_args!("{low} {}, {}", dst.low, rhs.low));
    asm::instruction(asm, format_args!("{high} {}, {}", dst.high, rhs.high));
    Ok(())
}

fn validate_pair_binary_assignment_ir(
    dst: &ir::RegisterPair,
    lhs: &ir::RegisterPair,
    rhs: &ir::RegisterPair,
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
    for (name, register) in [
        ("Pair arithmetic destination high register", &dst.high),
        ("Pair arithmetic destination low register", &dst.low),
        ("Pair arithmetic right high register", &rhs.high),
        ("Pair arithmetic right low register", &rhs.low),
    ] {
        validate_pair_binary_register(name, register)?;
    }
    Ok(())
}

fn emit_ir_wide_math_assignment(
    asm: &mut String,
    dst: &ir::RegisterPair,
    signed: bool,
    division: bool,
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let prefix = if division {
        "Widened division"
    } else {
        "Widened multiply"
    };
    if dst.high != "rdx" || dst.low != "rax" {
        return Err(format!(
            "{prefix} destination must be rdx:rax, found {}:{}",
            dst.high, dst.low
        ));
    }

    // Keep operand validation aligned with the established x86 diagnostics
    // while the machine emission itself consumes IR directly.
    validate_ir_wide_math_operand(
        &format!("{prefix} left operand"),
        lhs,
        strings,
        label_name,
        stack,
    )?;
    validate_ir_wide_math_operand(
        &format!("{prefix} right operand"),
        rhs,
        strings,
        label_name,
        stack,
    )?;

    let rhs = if is_ir_immediate(rhs)
        || ir_operand_uses_register_family(rhs, "rax")
        || ir_operand_uses_register_family(rhs, "rdx")
    {
        let temp = if !ir_operand_uses_register_family(lhs, "r10")
            && !ir_operand_uses_register_family(rhs, "r10")
        {
            "r10"
        } else if !ir_operand_uses_register_family(lhs, "r11")
            && !ir_operand_uses_register_family(rhs, "r11")
        {
            "r11"
        } else {
            return Err(format!(
                "{prefix} operands require an available scratch register"
            ));
        };
        let temp = ir::Operand::TargetRegister(temp.to_owned());
        emit_ir_copy_instruction(asm, rhs, &temp, strings, label_name, stack)?;
        temp
    } else {
        rhs.clone()
    };

    let rax = ir::Operand::TargetRegister(String::from("rax"));
    if lhs != &rax {
        emit_ir_copy_instruction(asm, lhs, &rax, strings, label_name, stack)?;
    }
    if division {
        asm::prepare_division(asm, signed);
    }
    let rhs = ir_machine_operand(&rhs, strings, label_name, stack)?;
    asm::wide_math(asm, wide_math_opcode(division, signed).to_owned(), rhs);
    Ok(())
}

fn is_ir_immediate(operand: &ir::Operand) -> bool {
    matches!(operand, ir::Operand::Immediate(_))
}

fn ir_operand_uses_register_family(operand: &ir::Operand, family: &str) -> bool {
    let uses = |register: &str| register_family(register) == register_family(family);
    match operand {
        ir::Operand::TargetRegister(register) => uses(register),
        ir::Operand::Memory { address, .. } | ir::Operand::AddressOf(address) => {
            ir_address_uses_register_family(address, &uses)
        }
        ir::Operand::Converted { operand, .. } | ir::Operand::Cast { operand, .. } => {
            ir_operand_uses_register_family(operand, family)
        }
        _ => false,
    }
}

fn ir_address_uses_register_family(address: &ir::Address, uses: &impl Fn(&str) -> bool) -> bool {
    let term_uses = |term: &ir::AddressTerm| match term {
        ir::AddressTerm::TargetRegister(register)
        | ir::AddressTerm::ScaledTargetRegister { register, .. } => uses(register),
        _ => false,
    };
    term_uses(&address.first) || address.rest.iter().any(|(_, term)| term_uses(term))
}

fn ir_condition_is_float(
    condition: &ir::Condition,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<bool, String> {
    Ok(matches!(condition, ir::Condition::Compare { .. })
        && resolve_ir_float_compare_width(condition, strings, label_name, stack)?.is_some())
}

fn resolve_ir_float_compare_width(
    condition: &ir::Condition,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<Option<MemoryWidth>, String> {
    let ir::Condition::Compare { lhs, op, rhs } = condition else {
        return Ok(None);
    };
    if let Some(width) = float_compare_width(*op) {
        return Ok(Some(width));
    }
    if !matches!(
        op,
        CompareOp::Equal
            | CompareOp::NotEqual
            | CompareOp::Less
            | CompareOp::LessEqual
            | CompareOp::Greater
            | CompareOp::GreaterEqual
    ) {
        return Ok(None);
    }

    let lhs_width = ir_operand_float_width(lhs, strings, label_name, stack);
    let rhs_width = ir_operand_float_width(rhs, strings, label_name, stack);
    match (lhs_width, rhs_width) {
        (Some(left), Some(right)) if left == right => Ok(Some(left)),
        (Some(left), None) if ir_can_use_float_context(rhs) => Ok(Some(left)),
        (None, Some(right)) if ir_can_use_float_context(lhs) => Ok(Some(right)),
        (Some(_), Some(_)) => Err(String::from(
            "Floating-point comparison operands must have matching widths",
        )),
        _ => Ok(None),
    }
}

fn ir_operand_float_width(
    operand: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Option<MemoryWidth> {
    match operand {
        ir::Operand::Memory { .. } | ir::Operand::Name(_) => {
            ir_operand_memory_width(operand, strings, stack).filter(|width| width.is_float())
        }
        _ => None,
    }
    .or_else(|| match operand {
        ir::Operand::Name(name) => strings
            .float_bindings
            .get(&(label_name.to_owned(), name.clone()))
            .map(|binding| binding.width),
        _ => None,
    })
}

fn ir_can_use_float_context(operand: &ir::Operand) -> bool {
    matches!(
        operand,
        ir::Operand::TargetRegister(_)
            | ir::Operand::FloatLiteral(_)
            | ir::Operand::Memory { .. }
            | ir::Operand::Name(_)
    )
}

fn validate_ir_float_compare_operand(
    name: &str,
    operand: &ir::Operand,
    width: MemoryWidth,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match operand {
        ir::Operand::Converted { .. } | ir::Operand::Cast { .. } => Err(format!(
            "{name} cannot use integer width conversion in floating-point math"
        )),
        ir::Operand::AddressOf(_) => Err(format!("{name} cannot be an address-of operand")),
        ir::Operand::TargetRegister(register) if is_xmm_register(register) => Ok(()),
        ir::Operand::FloatLiteral(value) => validate_float_literal(value, width),
        ir::Operand::Name(binding) if stack_scalar_slot(stack, binding).is_some() => {
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
        ir::Operand::Name(binding) => match strings
            .float_bindings
            .get(&(label_name.to_owned(), binding.clone()))
        {
            Some(float) if float.width == width => Ok(()),
            Some(_) => Err(format!(
                "{name} width must match the floating-point operator width"
            )),
            None => Err(format!("{name} cannot be a const or stack binding for now")),
        },
        ir::Operand::Memory { .. } => match ir_operand_memory_width(operand, strings, stack) {
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
        ir::Operand::Immediate(_) => Err(format!(
            "{name} cannot be an immediate value; use a floating-point memory operand for now"
        )),
        ir::Operand::StringProperty { .. } => Err(format!("{name} cannot be a string property")),
        ir::Operand::Pointer(_) => Err(format!("{name} cannot be an address-of operand")),
        ir::Operand::TargetRegister(register) => Err(format!(
            "{name} must be an XMM register, found integer register {register}"
        )),
    }
}

fn emit_ir_condition_jump(
    asm: &mut String,
    target: &str,
    condition: &ir::Condition,
    jump_if_true: bool,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    index: usize,
) -> Result<(), String> {
    match condition {
        ir::Condition::Compare { lhs, op, rhs } => {
            if let Some(width) =
                resolve_ir_float_compare_width(condition, strings, label_name, stack)?
            {
                return emit_ir_float_conditional_jump(
                    asm,
                    target,
                    lhs,
                    *op,
                    rhs,
                    width,
                    jump_if_true,
                    strings,
                    label_name,
                    stack,
                    index,
                );
            }
            let (lhs, rhs, op) = normalize_ir_compare(lhs, rhs, *op, strings, label_name)?;
            validate_resolved_integer_compare_op(op)?;
            validate_ir_condition_operand(lhs, strings, label_name, stack)?;
            validate_ir_condition_operand(rhs, strings, label_name, stack)?;
            let use_test = matches!(op, CompareOp::Equal | CompareOp::NotEqual)
                && matches!(lhs, ir::Operand::TargetRegister(register) if !is_xmm_register(register))
                && matches!(rhs, ir::Operand::Immediate(0));
            let lhs = emit_ir_operand(lhs, strings, label_name, stack)?;
            let rhs = emit_ir_operand(rhs, strings, label_name, stack)?;
            let op = if jump_if_true {
                op
            } else {
                invert_compare_op(op)
            };
            if use_test {
                asm::compare(
                    asm,
                    String::from("test"),
                    asm::Operand::Address(lhs.clone()),
                    asm::Operand::Address(lhs),
                );
            } else {
                asm::compare(
                    asm,
                    String::from("cmp"),
                    asm::Operand::Address(lhs),
                    asm::Operand::Address(rhs),
                );
            }
            asm::branch(
                asm,
                compare_jump_opcode(op).to_owned(),
                asm::Operand::Address(target.to_owned()),
            );
            Ok(())
        }
        ir::Condition::BitwiseAndZero { lhs, rhs, op } => {
            if !matches!(op, CompareOp::Equal | CompareOp::NotEqual) {
                return Err(String::from(
                    "Bitwise-and conditions only support == 0 or != 0",
                ));
            }
            validate_ir_condition_operand(lhs, strings, label_name, stack)?;
            validate_ir_condition_operand(rhs, strings, label_name, stack)?;
            let lhs = emit_ir_operand(lhs, strings, label_name, stack)?;
            let rhs = emit_ir_operand(rhs, strings, label_name, stack)?;
            let jump = match (*op, jump_if_true) {
                (CompareOp::Equal, true) | (CompareOp::NotEqual, false) => "je",
                (CompareOp::NotEqual, true) | (CompareOp::Equal, false) => "jne",
                _ => unreachable!(),
            };
            asm::compare(
                asm,
                String::from("test"),
                asm::Operand::Address(lhs),
                asm::Operand::Address(rhs),
            );
            asm::branch(
                asm,
                jump.to_owned(),
                asm::Operand::Address(target.to_owned()),
            );
            Ok(())
        }
    }
}

fn emit_ir_float_conditional_jump(
    asm: &mut String,
    target: &str,
    lhs: &ir::Operand,
    op: CompareOp,
    rhs: &ir::Operand,
    width: MemoryWidth,
    jump_if_true: bool,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    index: usize,
) -> Result<(), String> {
    validate_ir_float_compare_operand(
        "Floating-point comparison left operand",
        lhs,
        width,
        strings,
        label_name,
        stack,
    )?;
    validate_ir_float_compare_operand(
        "Floating-point comparison right operand",
        rhs,
        width,
        strings,
        label_name,
        stack,
    )?;

    if ir_operand_is_memory(lhs, stack) && ir_operand_is_memory(rhs, stack) {
        return Err(String::from(
            "Floating-point comparison cannot use memory for both operands",
        ));
    }

    let lhs = emit_ir_float_operand(lhs, width, strings, label_name, stack)?;
    let rhs = emit_ir_float_operand(rhs, width, strings, label_name, stack)?;
    let ordered_label = format!(".L.__subsea.{label_name}.fcmp_{index}_ordered");

    asm::instruction(
        asm,
        format_args!("{} {lhs}, {rhs}", float_compare_opcode(width)),
    );
    if jump_if_true {
        asm::branch(asm, "jp", asm::Operand::Address(ordered_label.clone()));
    } else {
        asm::branch(asm, "jp", asm::Operand::Address(target.to_owned()));
    }
    let op = if jump_if_true {
        op
    } else {
        invert_compare_op(op)
    };
    asm::branch(
        asm,
        float_compare_jump_opcode(op),
        asm::Operand::Address(target.to_owned()),
    );
    if jump_if_true {
        asm::label(asm, ordered_label);
    }
    Ok(())
}

fn normalize_ir_compare<'a>(
    lhs: &'a ir::Operand,
    rhs: &'a ir::Operand,
    op: CompareOp,
    strings: &StringTable,
    label_name: &str,
) -> Result<(&'a ir::Operand, &'a ir::Operand, CompareOp), String> {
    let lhs_immediate = ir_operand_immediate_value(lhs, strings, label_name).is_some();
    let rhs_immediate = ir_operand_immediate_value(rhs, strings, label_name).is_some();
    if lhs_immediate {
        if rhs_immediate {
            return Err(String::from("Comparison cannot use two immediate operands"));
        }
        Ok((rhs, lhs, reverse_compare_op(op)))
    } else {
        Ok((lhs, rhs, op))
    }
}

fn emit_ir_call_instruction(
    asm: &mut String,
    target: &ir::ControlTarget,
    labels: &LabelSymbols,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match target {
        ir::ControlTarget::Label(target) => {
            asm::call(asm, asm::Operand::Address(labels.emit_label(target)));
            Ok(())
        }
        ir::ControlTarget::Operand(operand) => {
            validate_ir_indirect_control_target("call", operand, strings, label_name, stack)?;
            let operand = emit_ir_operand(operand, strings, label_name, stack)?;
            asm::call(asm, asm::Operand::Address(operand));
            Ok(())
        }
    }
}

fn validate_ir_indirect_control_target(
    instruction: &str,
    operand: &ir::Operand,
    _strings: &StringTable,
    _label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let width = match operand {
        ir::Operand::TargetRegister(register) => {
            if is_xmm_register(register) {
                return Err(format!(
                    "indirect {instruction} target must be a 64-bit integer register or memory operand"
                ));
            }
            register_width(register)
        }
        ir::Operand::Memory { width, .. } => width.map(memory_width_bits),
        ir::Operand::Name(name) => {
            stack_scalar_slot(stack, name).map(|(_, width)| memory_width_bits(width))
        }
        _ => None,
    };

    match width {
        Some(Width::Bits64) => Ok(()),
        Some(width) => Err(format!(
            "indirect {instruction} target must be 64-bit, found {}-bit operand",
            width.bits()
        )),
        None => Err(format!(
            "indirect {instruction} target must be a 64-bit register or memory operand"
        )),
    }
}

fn emit_ir_return_instruction(
    asm: &mut String,
    instruction: &ir::Instruction,
    stack: &StackFrame,
    spec: TargetSpec,
) {
    if !matches!(instruction, ir::Instruction::Ret) {
        return;
    }

    if stack.has_slots() {
        emit_frame_epilogue(asm, spec);
    }
    asm::ret(asm);
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

fn float_math_op_from_integer_op(op: MathOp) -> FloatMathOp {
    match op {
        MathOp::Add => FloatMathOp::Add,
        MathOp::Multiply => FloatMathOp::Multiply,
        MathOp::Subtract => FloatMathOp::Subtract,
        _ => unreachable!(),
    }
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

fn emit_data(asm: &mut String, memory: &[ir::MemoryDeclaration], labels: &LabelSymbols) {
    if memory
        .iter()
        .all(|declaration| matches!(declaration, ir::MemoryDeclaration::Buffer { .. }))
    {
        return;
    }

    asm::section(asm, "data");

    for declaration in memory {
        match declaration {
            ir::MemoryDeclaration::Scalar { name, width, value } => {
                asm::label(asm, name);
                asm::scalar(asm, width.directive(), format_data_scalar(*width, *value));
            }
            ir::MemoryDeclaration::FloatScalar { name, width, value } => {
                asm::label(asm, name);
                asm::scalar(asm, width.directive(), value);
            }
            ir::MemoryDeclaration::Array {
                name,
                width,
                values,
            } => {
                asm::label(asm, name);
                emit_memory_values(asm, *width, values, labels);
            }
            ir::MemoryDeclaration::Repeat {
                name,
                width,
                count,
                value,
            } => {
                asm::label(asm, name);
                for _ in 0..*count {
                    emit_memory_value(asm, *width, value, labels);
                }
            }
            ir::MemoryDeclaration::Buffer { .. } => {}
        }
    }

    asm.push('\n');
}

fn emit_memory_values(
    asm: &mut String,
    width: MemoryWidth,
    values: &[ir::MemoryValue],
    labels: &LabelSymbols,
) {
    for value in values {
        emit_memory_value(asm, width, value, labels);
    }
}

fn emit_memory_value(
    asm: &mut String,
    width: MemoryWidth,
    value: &ir::MemoryValue,
    labels: &LabelSymbols,
) {
    match value {
        ir::MemoryValue::Integer(value) => {
            asm::scalar(asm, width.directive(), format_data_scalar(width, *value));
        }
        ir::MemoryValue::Address { target } => {
            asm::quad(asm, labels.emit_label(target));
        }
    }
}

fn emit_static_data(asm: &mut String, data: &[ir::DataDeclaration], labels: &LabelSymbols) {
    for declaration in data {
        let flags = if declaration.keep { "aR" } else { "a" };
        asm::top_level_directive(
            asm,
            format_args!(".section {}, \"{}\", @progbits", declaration.section, flags),
        );

        if declaration.export {
            asm::global(asm, &declaration.name);
        }

        if let Some(align) = declaration.align {
            asm::top_level_directive(asm, format_args!(".balign {align}"));
        }

        asm::label(asm, &declaration.name);

        for item in &declaration.items {
            match item {
                ir::DataItem::Scalar { width, value } => {
                    asm::scalar(asm, width.directive(), format_data_scalar(*width, *value));
                }
                ir::DataItem::Address { target } => {
                    asm::quad(asm, labels.emit_label(target));
                }
                ir::DataItem::Zero { count } => {
                    asm::zero(asm, count);
                }
                ir::DataItem::Label { name } => {
                    asm::label(asm, name);
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

fn emit_bss(asm: &mut String, memory: &[ir::MemoryDeclaration]) {
    let buffers: Vec<_> = memory
        .iter()
        .filter_map(|declaration| match declaration {
            ir::MemoryDeclaration::Scalar { .. }
            | ir::MemoryDeclaration::FloatScalar { .. }
            | ir::MemoryDeclaration::Array { .. }
            | ir::MemoryDeclaration::Repeat { .. } => None,
            ir::MemoryDeclaration::Buffer { name, width, count } => Some((name, width, count)),
        })
        .collect();

    if buffers.is_empty() {
        return;
    }

    asm::section(asm, "bss");

    for (name, width, count) in buffers {
        asm::label(asm, name);
        asm::zero(asm, width.size() * count);
    }

    asm.push('\n');
}

fn emit_rodata(asm: &mut String, strings: &[StringBinding], floats: &[FloatBinding]) {
    if strings.is_empty() && floats.is_empty() {
        return;
    }

    let mut bindings: Vec<_> = strings.iter().collect();
    bindings.sort_by(|left, right| left.asm_label.cmp(&right.asm_label));

    asm::section(asm, "rodata");

    for string in bindings {
        asm::label(asm, &string.asm_label);

        if string.value.is_empty() {
            asm::byte(asm, 0);
        } else {
            let bytes = string
                .value
                .as_bytes()
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(", ");

            asm::byte(asm, bytes);
        }
    }

    let mut float_bindings: Vec<_> = floats.iter().collect();
    float_bindings.sort_by(|left, right| left.asm_label.cmp(&right.asm_label));

    for float in float_bindings {
        asm::label(asm, &float.asm_label);
        asm::scalar(asm, float.width.directive(), &float.value);
    }

    asm.push('\n');
}

fn emit_print_string_instruction(asm: &mut String, string: &StringBinding) {
    emit_print_volatile_pushes(asm);
    emit_linux_write_label(asm, &string.asm_label, string.value.len());
    emit_print_volatile_pops(asm);
}

fn resolve_ir_print_part<'a>(
    strings: &'a StringTable,
    literal_indexes: &mut HashMap<String, usize>,
    label_name: &str,
    part: &ir::PrintPart,
) -> Result<&'a StringBinding, String> {
    match part {
        ir::PrintPart::Binding(name) => strings
            .bindings
            .get(&(label_name.to_owned(), name.clone()))
            .ok_or_else(|| {
                format!("Cannot print unknown binding {name:?} in label {label_name:?}")
            }),
        ir::PrintPart::Literal(_) => {
            let index = literal_indexes.entry(label_name.to_owned()).or_insert(0);
            *index += 1;
            strings
                .literals
                .get(&(label_name.to_owned(), *index))
                .ok_or_else(|| String::from("Internal error: missing print literal"))
        }
        ir::PrintPart::Operand(_) | ir::PrintPart::FormattedOperand { .. } => {
            Err(String::from("Internal error: operand print is runtime"))
        }
    }
}

fn emit_print_volatile_pushes(asm: &mut String) {
    for register in ["rax", "rcx", "rdi", "rsi", "rdx", "r11"] {
        asm::push(asm, asm::Operand::Register(String::from(register)));
    }
}

fn emit_print_volatile_pops(asm: &mut String) {
    for register in ["r11", "rdx", "rsi", "rdi", "rcx", "rax"] {
        asm::pop(asm, asm::Operand::Register(String::from(register)));
    }
}

fn emit_print_operand_instruction(
    asm: &mut String,
    operand: &ir::Operand,
    format: ir::PrintFormat,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    index: usize,
) -> Result<(), BackendError> {
    let format = resolve_ir_print_format(format, operand, strings, label_name, stack)?;

    if ir_operand_is_float(operand, strings, label_name, stack) {
        return Err(BackendError::new(
            "print operand does not support floating-point values yet",
        ));
    }

    if matches!(operand, ir::Operand::Pointer(_)) {
        return Err(BackendError::new(
            "print operand cannot be an address-of operand",
        ));
    }

    if ir_operand_uses_high_byte(operand) {
        return Err(BackendError::new(
            "print operand cannot use high-byte registers ah, bh, ch, or dh",
        ));
    }

    validate_ir_print_format_operand(format, operand, strings, label_name, stack)?;

    emit_print_volatile_pushes(asm);
    load_ir_print_operand(asm, operand, format, strings, label_name, stack)?;

    let loop_label = format!(".L.__subsea.{label_name}.print_{index}_loop");
    let negative_label = format!(".L.__subsea.{label_name}.print_{index}_negative");
    let digits_label = format!(".L.__subsea.{label_name}.print_{index}_digits");
    let prefix_done_label = format!(".L.__subsea.{label_name}.print_{index}_prefix_done");
    let digit_decimal_label = format!(".L.__subsea.{label_name}.print_{index}_digit_decimal");

    asm::push(asm, asm::Operand::Register(String::from("rbx")));
    asm::stack_adjust(asm, String::from("sub"), String::from("rsp"), 80);
    asm::lea(
        asm,
        asm::Operand::Register(String::from("rsi")),
        String::from("[rsp + 80]"),
    );
    match format {
        ir::PrintFormat::Infer => unreachable!(),
        ir::PrintFormat::SignedDecimal(_) => {
            asm::mov(
                asm,
                asm::Operand::Register(String::from("rbx")),
                asm::Operand::Immediate(10),
            );
            asm::mov(
                asm,
                asm::Operand::Address(String::from("byte ptr [rsp]")),
                asm::Operand::Immediate(0),
            );
            asm::compare(
                asm,
                String::from("cmp"),
                asm::Operand::Register(String::from("rax")),
                asm::Operand::Immediate(0),
            );
            asm::branch(
                asm,
                String::from("jl"),
                asm::Operand::Address(negative_label.clone()),
            );
            asm::jump(asm, asm::Operand::Address(digits_label.clone()));
            asm::label(asm, negative_label.clone());
            asm::unary(
                asm,
                String::from("neg"),
                asm::Operand::Register(String::from("rax")),
            );
            asm::mov(
                asm,
                asm::Operand::Address(String::from("byte ptr [rsp]")),
                asm::Operand::Immediate(45),
            );
        }
        ir::PrintFormat::UnsignedDecimal(_) => {
            asm::mov(
                asm,
                asm::Operand::Register(String::from("rbx")),
                asm::Operand::Immediate(10),
            );
        }
        ir::PrintFormat::Hex | ir::PrintFormat::Pointer => {
            asm::mov(
                asm,
                asm::Operand::Register(String::from("rbx")),
                asm::Operand::Immediate(16),
            );
            asm::mov(
                asm,
                asm::Operand::Address(String::from("byte ptr [rsp]")),
                asm::Operand::Immediate(48),
            );
            asm::mov(
                asm,
                asm::Operand::Address(String::from("byte ptr [rsp + 1]")),
                asm::Operand::Immediate(120),
            );
        }
        ir::PrintFormat::Binary => {
            asm::mov(
                asm,
                asm::Operand::Register(String::from("rbx")),
                asm::Operand::Immediate(2),
            );
            asm::mov(
                asm,
                asm::Operand::Address(String::from("byte ptr [rsp]")),
                asm::Operand::Immediate(48),
            );
            asm::mov(
                asm,
                asm::Operand::Address(String::from("byte ptr [rsp + 1]")),
                asm::Operand::Immediate(98),
            );
        }
    }
    asm::label(asm, digits_label.clone());
    asm::label(asm, loop_label.clone());
    asm::binary(
        asm,
        String::from("xor"),
        asm::Operand::Register(String::from("rdx")),
        asm::Operand::Register(String::from("rdx")),
    );
    asm::wide_math(
        asm,
        String::from("div"),
        asm::Operand::Register(String::from("rbx")),
    );
    if matches!(format, ir::PrintFormat::Hex | ir::PrintFormat::Pointer) {
        asm::compare(
            asm,
            String::from("cmp"),
            asm::Operand::Register(String::from("dl")),
            asm::Operand::Immediate(9),
        );
        asm::branch(
            asm,
            String::from("jbe"),
            asm::Operand::Address(digit_decimal_label.clone()),
        );
        asm::binary(
            asm,
            String::from("add"),
            asm::Operand::Register(String::from("dl")),
            asm::Operand::Immediate(87),
        );
        asm::jump(asm, asm::Operand::Address(prefix_done_label.clone()));
        asm::label(asm, digit_decimal_label.clone());
        asm::binary(
            asm,
            String::from("add"),
            asm::Operand::Register(String::from("dl")),
            asm::Operand::Immediate(48),
        );
        asm::label(asm, prefix_done_label.clone());
    } else {
        asm::binary(
            asm,
            String::from("add"),
            asm::Operand::Register(String::from("dl")),
            asm::Operand::Immediate(48),
        );
    }
    asm::binary(
        asm,
        String::from("sub"),
        asm::Operand::Register(String::from("rsi")),
        asm::Operand::Immediate(1),
    );
    asm::mov(
        asm,
        asm::Operand::Address(String::from("byte ptr [rsi]")),
        asm::Operand::Register(String::from("dl")),
    );
    asm::compare(
        asm,
        String::from("cmp"),
        asm::Operand::Register(String::from("rax")),
        asm::Operand::Immediate(0),
    );
    asm::branch(
        asm,
        String::from("jne"),
        asm::Operand::Address(loop_label.clone()),
    );
    match format {
        ir::PrintFormat::Infer => unreachable!(),
        ir::PrintFormat::SignedDecimal(_) => {
            asm::compare(
                asm,
                String::from("cmp"),
                asm::Operand::Address(String::from("byte ptr [rsp]")),
                asm::Operand::Immediate(45),
            );
            asm::branch(
                asm,
                String::from("jne"),
                asm::Operand::Address(prefix_done_label.clone()),
            );
            asm::binary(
                asm,
                String::from("sub"),
                asm::Operand::Register(String::from("rsi")),
                asm::Operand::Immediate(1),
            );
            asm::mov(
                asm,
                asm::Operand::Address(String::from("byte ptr [rsi]")),
                asm::Operand::Immediate(45),
            );
            asm::label(asm, prefix_done_label.clone());
        }
        ir::PrintFormat::Hex | ir::PrintFormat::Pointer | ir::PrintFormat::Binary => {
            let marker = match format {
                ir::PrintFormat::Infer => unreachable!(),
                ir::PrintFormat::Binary => 98,
                _ => 120,
            };
            for value in [marker, 48] {
                asm::binary(
                    asm,
                    String::from("sub"),
                    asm::Operand::Register(String::from("rsi")),
                    asm::Operand::Immediate(1),
                );
                asm::mov(
                    asm,
                    asm::Operand::Address(String::from("byte ptr [rsi]")),
                    asm::Operand::Immediate(value),
                );
            }
        }
        ir::PrintFormat::UnsignedDecimal(_) => {}
    }
    asm::lea(
        asm,
        asm::Operand::Register(String::from("rdx")),
        String::from("[rsp + 80]"),
    );
    asm::binary(
        asm,
        String::from("sub"),
        asm::Operand::Register(String::from("rdx")),
        asm::Operand::Register(String::from("rsi")),
    );
    emit_linux_write_registers(asm);
    asm::stack_adjust(asm, String::from("add"), String::from("rsp"), 80);
    asm::pop(asm, asm::Operand::Register(String::from("rbx")));
    emit_print_volatile_pops(asm);

    Ok(())
}

fn validate_ir_print_format_operand(
    format: ir::PrintFormat,
    operand: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(operand, ir::Operand::Immediate(_)) {
        return Ok(());
    }

    if let Some(expected) = print_format_operand_width(format) {
        return match ir_operand_width(operand, strings, label_name, stack) {
            Some(width) if width == memory_width_bits(expected) => Ok(()),
            Some(width) => Err(format!(
                "{} print operand must be {}-bit, found {}-bit operand",
                ir_print_format_name(format),
                memory_width_bits(expected).bits(),
                width.bits()
            )),
            None => Err(format!(
                "{} print operand must have a known {}-bit width",
                ir_print_format_name(format),
                memory_width_bits(expected).bits()
            )),
        };
    }

    match ir_operand_width(operand, strings, label_name, stack) {
        Some(Width::Bits64) => Ok(()),
        Some(width) => Err(format!(
            "{} print operand must be 64-bit, found {}-bit operand",
            ir_print_format_name(format),
            width.bits()
        )),
        None => Err(format!(
            "{} print operand must be an integer immediate, const, 64-bit register, or 64-bit memory operand",
            ir_print_format_name(format)
        )),
    }
}

fn print_format_operand_width(format: ir::PrintFormat) -> Option<MemoryWidth> {
    match format {
        ir::PrintFormat::SignedDecimal(width) | ir::PrintFormat::UnsignedDecimal(width) => {
            Some(width)
        }
        ir::PrintFormat::Pointer => Some(MemoryWidth::Ptr),
        ir::PrintFormat::Hex | ir::PrintFormat::Binary | ir::PrintFormat::Infer => None,
    }
}

fn resolve_ir_print_format(
    format: ir::PrintFormat,
    operand: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<ir::PrintFormat, String> {
    if format != ir::PrintFormat::Infer {
        return Ok(format);
    }

    match operand {
        ir::Operand::Immediate(_) => Ok(ir::PrintFormat::SignedDecimal(MemoryWidth::I64)),
        ir::Operand::Name(name) => {
            if let Some((_, width)) = stack_scalar_slot(stack, name) {
                return Ok(infer_ir_print_format_for_width(width));
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
                    .map(infer_ir_print_format_for_width)
                    .unwrap_or(ir::PrintFormat::SignedDecimal(MemoryWidth::I64)));
            }

            Err(format!(
                "Cannot infer print format for {name:?}; use {{i64}}, {{u64}}, {{x}}, {{b}}, or {{ptr}}"
            ))
        }
        ir::Operand::Memory { .. } => {
            let Some(width) = ir_operand_memory_width(operand, strings, stack) else {
                return Err(String::from(
                    "Cannot infer print format for memory operand without a known width; use {i64}, {u64}, {x}, {b}, or {ptr}",
                ));
            };

            if width.is_float() {
                return Err(String::from(
                    "print operand does not support floating-point values yet",
                ));
            }

            Ok(infer_ir_print_format_for_width(width))
        }
        ir::Operand::StringProperty { property, .. } => match property {
            ir::StringProperty::Len => Ok(ir::PrintFormat::UnsignedDecimal(MemoryWidth::U64)),
            ir::StringProperty::Ptr => Ok(ir::PrintFormat::Pointer),
        },
        ir::Operand::TargetRegister(register) => Err(format!(
            "Cannot infer print format for register {register}; use {{i64}}, {{u64}}, {{x}}, {{b}}, or {{ptr}}"
        )),
        _ => Err(String::from(
            "Cannot infer print format for this operand; use {i64}, {u64}, {x}, {b}, or {ptr}",
        )),
    }
}

fn infer_ir_print_format_for_width(width: MemoryWidth) -> ir::PrintFormat {
    match width {
        MemoryWidth::I8 | MemoryWidth::I16 | MemoryWidth::I32 | MemoryWidth::I64 => {
            ir::PrintFormat::SignedDecimal(width)
        }
        MemoryWidth::U8 | MemoryWidth::U16 | MemoryWidth::U32 | MemoryWidth::U64 => {
            ir::PrintFormat::UnsignedDecimal(width)
        }
        MemoryWidth::Ptr => ir::PrintFormat::Pointer,
        MemoryWidth::F32 | MemoryWidth::F64 => ir::PrintFormat::Infer,
    }
}

fn ir_print_format_name(format: ir::PrintFormat) -> &'static str {
    match format {
        ir::PrintFormat::Infer => "inferred",
        ir::PrintFormat::SignedDecimal(MemoryWidth::I8) => "i8",
        ir::PrintFormat::SignedDecimal(MemoryWidth::I16) => "i16",
        ir::PrintFormat::SignedDecimal(MemoryWidth::I32) => "i32",
        ir::PrintFormat::SignedDecimal(MemoryWidth::I64) => "i64",
        ir::PrintFormat::UnsignedDecimal(MemoryWidth::U8) => "u8",
        ir::PrintFormat::UnsignedDecimal(MemoryWidth::U16) => "u16",
        ir::PrintFormat::UnsignedDecimal(MemoryWidth::U32) => "u32",
        ir::PrintFormat::UnsignedDecimal(MemoryWidth::U64) => "u64",
        ir::PrintFormat::SignedDecimal(_) | ir::PrintFormat::UnsignedDecimal(_) => "integer",
        ir::PrintFormat::Hex => "hex",
        ir::PrintFormat::Binary => "binary",
        ir::PrintFormat::Pointer => "pointer",
    }
}

fn emit_print_stack_string_instruction(
    asm: &mut String,
    name: &str,
    stack: &StackFrame,
) -> Result<(), BackendError> {
    let (ptr_offset, len_offset) = stack_string_slot(stack, name)
        .ok_or_else(|| format!("Unknown string stack variable {name:?}"))?;

    emit_print_volatile_pushes(asm);
    asm::load(
        asm,
        asm::Operand::Register(String::from("rsi")),
        asm::Operand::Address(format!("qword ptr [rbp - {ptr_offset}]")),
    );
    asm::load(
        asm,
        asm::Operand::Register(String::from("rdx")),
        asm::Operand::Address(format!("qword ptr [rbp - {len_offset}]")),
    );
    emit_linux_write_registers(asm);
    emit_print_volatile_pops(asm);

    Ok(())
}

fn emit_stack_string_address(asm: &mut String, label: &str, ptr_offset: usize) {
    asm::push(asm, asm::Operand::Register(String::from("r10")));
    asm::lea(
        asm,
        asm::Operand::Register(String::from("r10")),
        format!("[rip + {label}]"),
    );
    asm::mov(
        asm,
        asm::Operand::Address(format!("qword ptr [rbp - {ptr_offset}]")),
        asm::Operand::Register(String::from("r10")),
    );
    asm::pop(asm, asm::Operand::Register(String::from("r10")));
}

fn emit_ir_stack_string_slice_pointer(
    asm: &mut String,
    ptr: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    ptr_offset: usize,
) -> Result<(), String> {
    match ptr {
        ir::Operand::Pointer(name) => {
            if let Some((buffer_offset, _)) = stack_buffer_slot(stack, name) {
                asm::push(asm, asm::Operand::Register(String::from("r10")));
                asm::lea(
                    asm,
                    asm::Operand::Register(String::from("r10")),
                    format!("[rbp - {buffer_offset}]"),
                );
                asm::mov(
                    asm,
                    asm::Operand::Address(format!("qword ptr [rbp - {ptr_offset}]")),
                    asm::Operand::Register(String::from("r10")),
                );
                asm::pop(asm, asm::Operand::Register(String::from("r10")));
            } else {
                emit_stack_string_address(asm, name, ptr_offset);
            }
            Ok(())
        }
        ir::Operand::AddressOf(address) => {
            asm::push(asm, asm::Operand::Register(String::from("r10")));
            asm::lea(
                asm,
                asm::Operand::Register(String::from("r10")),
                format!("[{}]", emit_ir_address(address, stack)),
            );
            asm::mov(
                asm,
                asm::Operand::Address(format!("qword ptr [rbp - {ptr_offset}]")),
                asm::Operand::Register(String::from("r10")),
            );
            asm::pop(asm, asm::Operand::Register(String::from("r10")));
            Ok(())
        }
        ir::Operand::TargetRegister(name) => match register_width(name) {
            Some(Width::Bits64) => {
                asm::mov(
                    asm,
                    asm::Operand::Address(format!("qword ptr [rbp - {ptr_offset}]")),
                    asm::Operand::Register(name.clone()),
                );
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
            let operand = emit_ir_operand(operand, strings, label_name, stack)?;
            Err(format!(
                "slice pointer must be a 64-bit register or address-of operand, found {operand}"
            ))
        }
    }
}

fn emit_ir_stack_string_slice_len(
    asm: &mut String,
    len: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
    len_offset: usize,
) -> Result<(), String> {
    if let Some(value) = ir_operand_immediate_value(len, strings, label_name) {
        validate_immediate_range(value, ImmediateDestination::Memory(MemoryWidth::U64))?;
        asm::mov(
            asm,
            asm::Operand::Address(format!("qword ptr [rbp - {len_offset}]")),
            asm::Operand::Immediate(value),
        );
        return Ok(());
    }

    match ir_operand_width(len, strings, label_name, stack) {
        Some(Width::Bits64) => {
            let emitted_len = emit_ir_operand(len, strings, label_name, stack)?;
            if ir_operand_is_memory(len, stack) {
                asm::push(asm, asm::Operand::Register(String::from("r10")));
                asm::mov(
                    asm,
                    asm::Operand::Register(String::from("r10")),
                    asm::Operand::Address(emitted_len),
                );
                asm::mov(
                    asm,
                    asm::Operand::Address(format!("qword ptr [rbp - {len_offset}]")),
                    asm::Operand::Register(String::from("r10")),
                );
                asm::pop(asm, asm::Operand::Register(String::from("r10")));
            } else {
                asm::mov(
                    asm,
                    asm::Operand::Address(format!("qword ptr [rbp - {len_offset}]")),
                    asm::Operand::Address(emitted_len),
                );
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
    src: &ir::ReadSource,
    dst: &ir::Operand,
    len: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), BackendError> {
    emit_read_len_arg(asm, len, strings, label_name, stack)?;
    emit_read_dst_arg(asm, dst, stack)?;
    emit_read_src_arg(asm, src);
    emit_linux_read(asm);

    Ok(())
}

fn emit_release_instruction(
    asm: &mut String,
    ptr: &ir::Operand,
    len: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), BackendError> {
    emit_release_ptr_arg(asm, ptr, strings, label_name, stack)?;
    emit_release_len_arg(asm, len, strings, label_name, stack)?;
    emit_linux_munmap(asm);

    Ok(())
}

fn emit_release_ptr_arg(
    asm: &mut String,
    ptr: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    emit_ir_linux_memory_pointer_arg(asm, ptr, strings, label_name, stack)
}

fn emit_release_len_arg(
    asm: &mut String,
    len: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(len, ir::Operand::TargetRegister(register) if register == "rdi") {
        return Err(String::from(
            "release size cannot use rdi because release uses rdi for the pointer",
        ));
    }

    emit_ir_linux_memory_size_arg(asm, "rsi", "release size", len, strings, label_name, stack)
}

fn emit_ir_linux_memory_pointer_arg(
    asm: &mut String,
    ptr: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match ptr {
        ir::Operand::Pointer(name) => {
            if stack_buffer_slot(stack, name).is_some() {
                return Err(String::from(
                    "release pointer cannot refer to a stack byte buffer",
                ));
            }
            asm::lea(
                asm,
                asm::Operand::Register(String::from("rdi")),
                format!("[rip + {name}]"),
            );
            Ok(())
        }
        _ => match ir_operand_width(ptr, strings, label_name, stack) {
            Some(Width::Bits64) => {
                let ptr = emit_ir_operand(ptr, strings, label_name, stack)?;
                asm::mov(
                    asm,
                    asm::Operand::Register(String::from("rdi")),
                    asm::Operand::Address(ptr),
                );
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

fn emit_ir_linux_memory_size_arg(
    asm: &mut String,
    dst_register: &str,
    description: &str,
    len: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if let Some(value) = ir_operand_immediate_value(len, strings, label_name) {
        validate_immediate_range(value, ImmediateDestination::Memory(MemoryWidth::U64))?;
        asm::mov(
            asm,
            asm::Operand::Register(dst_register.to_owned()),
            asm::Operand::Immediate(value),
        );
        return Ok(());
    }

    match ir_operand_width(len, strings, label_name, stack) {
        Some(Width::Bits64) => {
            let len = emit_ir_operand(len, strings, label_name, stack)?;
            asm::mov(
                asm,
                asm::Operand::Register(dst_register.to_owned()),
                asm::Operand::Address(len),
            );
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

fn emit_read_src_arg(asm: &mut String, src: &ir::ReadSource) {
    match src {
        ir::ReadSource::Stdin => asm::mov(
            asm,
            asm::Operand::Register(String::from("rdi")),
            asm::Operand::Immediate(linux::STDIN as i128),
        ),
    }
}

fn emit_read_dst_arg(
    asm: &mut String,
    dst: &ir::Operand,
    stack: &StackFrame,
) -> Result<(), String> {
    match dst {
        ir::Operand::Pointer(name) => {
            if let Some((offset, _)) = stack_buffer_slot(stack, name) {
                asm::lea(
                    asm,
                    asm::Operand::Register(String::from("rsi")),
                    format!("[rbp - {offset}]"),
                );
            } else {
                asm::lea(
                    asm,
                    asm::Operand::Register(String::from("rsi")),
                    format!("[rip + {name}]"),
                );
            }
            Ok(())
        }
        ir::Operand::TargetRegister(name) => {
            if name == "rdx" {
                return Err(String::from(
                    "read destination cannot use rdx because read uses rdx for the buffer size",
                ));
            }

            match register_width(name) {
                Some(Width::Bits64) => {
                    asm::mov(
                        asm,
                        asm::Operand::Register(String::from("rsi")),
                        asm::Operand::Register(name.clone()),
                    );
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
    len: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if let Some(value) = ir_operand_immediate_value(len, strings, label_name) {
        validate_immediate_range(value, ImmediateDestination::Memory(MemoryWidth::U64))?;
        asm::mov(
            asm,
            asm::Operand::Register(String::from("rdx")),
            asm::Operand::Immediate(value),
        );
        return Ok(());
    }

    match ir_operand_width(len, strings, label_name, stack) {
        Some(Width::Bits64) => {
            let len = emit_ir_operand(len, strings, label_name, stack)?;
            asm::mov(
                asm,
                asm::Operand::Register(String::from("rdx")),
                asm::Operand::Address(len),
            );
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

fn load_ir_print_operand(
    asm: &mut String,
    operand: &ir::Operand,
    format: ir::PrintFormat,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let operand = emit_ir_operand(operand, strings, label_name, stack)?;
    match format {
        ir::PrintFormat::SignedDecimal(MemoryWidth::I8)
        | ir::PrintFormat::SignedDecimal(MemoryWidth::I16) => {
            asm::instruction(asm, format_args!("movsx rax, {operand}"));
        }
        ir::PrintFormat::SignedDecimal(MemoryWidth::I32) => {
            asm::instruction(asm, format_args!("movsxd rax, {operand}"));
        }
        ir::PrintFormat::SignedDecimal(MemoryWidth::I64) => {
            asm::instruction(asm, format_args!("mov rax, {operand}"));
        }
        ir::PrintFormat::UnsignedDecimal(MemoryWidth::U8) => {
            asm::instruction(asm, format_args!("movzx rax, {operand}"));
        }
        ir::PrintFormat::UnsignedDecimal(MemoryWidth::U16) => {
            asm::instruction(asm, format_args!("movzx rax, {operand}"));
        }
        ir::PrintFormat::UnsignedDecimal(MemoryWidth::U32) => {
            asm::instruction(asm, format_args!("mov eax, {operand}"));
        }
        ir::PrintFormat::UnsignedDecimal(MemoryWidth::U64)
        | ir::PrintFormat::Hex
        | ir::PrintFormat::Binary
        | ir::PrintFormat::Pointer => {
            asm::instruction(asm, format_args!("mov rax, {operand}"));
        }
        ir::PrintFormat::Infer
        | ir::PrintFormat::SignedDecimal(_)
        | ir::PrintFormat::UnsignedDecimal(_) => unreachable!(),
    }
    Ok(())
}

fn emit_ir_assignment(
    asm: &mut String,
    instruction: &ir::Instruction,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let ir::Instruction::Assign { dst, value } = instruction else {
        return Err(String::from("Expected an IR assignment"));
    };

    match value {
        ir::Value::Operand(src) => {
            emit_ir_operand_assignment(asm, dst, src, strings, label_name, stack)
        }
        ir::Value::Binary { op, lhs, rhs } => {
            emit_ir_binary_value_assignment(asm, dst, *op, lhs, rhs, strings, label_name, stack)
        }
        ir::Value::Expression { op, lhs, rhs } => {
            emit_ir_expression_assignment(asm, dst, *op, lhs, rhs, strings, label_name, stack)
        }
        ir::Value::BitwiseUnary { op, operand } => {
            emit_ir_bitwise_assignment(asm, dst, *op, operand, strings, label_name, stack)
        }
        ir::Value::FloatBinary {
            width,
            op,
            lhs,
            rhs,
        } => emit_ir_float_binary_assignment(
            asm, dst, *width, *op, lhs, rhs, strings, label_name, stack,
        ),
        ir::Value::Condition(condition) => {
            if ir_condition_is_float(condition, strings, label_name, stack)? {
                return Err(String::from(
                    "Boolean assignment does not support floating-point comparisons yet",
                ));
            }
            emit_ir_boolean_condition_assignment(asm, dst, condition, strings, label_name, stack)
        }
        ir::Value::IntrinsicCall { op, width, args } => emit_ir_intrinsic_call_assignment(
            asm, dst, *op, *width, args, strings, label_name, stack,
        ),
        ir::Value::StringBytes { value } => emit_ir_string_bytes_assignment(asm, dst, value, stack),
        ir::Value::PlatformReserve { len } => {
            emit_ir_platform_reserve_assignment(asm, dst, len, strings, label_name, stack)
        }
    }
}

fn emit_ir_operand_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    src: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if let ir::Operand::Name(name) = src {
        let key = (label_name.to_owned(), name.clone());
        if !strings.integers.contains_key(&key)
            && !strings.float_bindings.contains_key(&key)
            && let Some(binding) = strings.bindings.get(&key)
        {
            if !matches!(dst, ir::Operand::Memory { .. }) {
                return Err(format!(
                    "String binding {name:?} in label {label_name:?} cannot be used as an operand"
                ));
            }
            if binding.value.is_empty() {
                return Err(String::from("String byte assignment cannot be empty"));
            }
            return emit_ir_string_bytes_assignment(asm, dst, &binding.value, stack);
        }
    }
    if matches!(
        src,
        ir::Operand::Pointer(_)
            | ir::Operand::AddressOf(_)
            | ir::Operand::Converted { .. }
            | ir::Operand::Cast { .. }
    ) {
        return emit_ir_special_copy(asm, src, dst, strings, label_name, stack);
    }
    if ir_operand_is_float(src, strings, label_name, stack)
        || ir_operand_is_float(dst, strings, label_name, stack)
    {
        if matches!(src, ir::Operand::TargetRegister(name) if is_xmm_register(name))
            && matches!(dst, ir::Operand::TargetRegister(name) if is_xmm_register(name))
        {
            let (ir::Operand::TargetRegister(src), ir::Operand::TargetRegister(dst)) = (src, dst)
            else {
                unreachable!()
            };
            asm::instruction(asm, format_args!("movaps {dst}, {src}"));
            return Ok(());
        }
        validate_ir_copy_assignment(src, dst, strings, label_name, stack)?;
        return emit_ir_float_copy_instruction(
            asm,
            src,
            dst,
            ir_float_width(src, dst, strings, label_name, stack)?,
            strings,
            label_name,
            stack,
        );
    }
    if matches!(src, ir::Operand::TargetRegister(_))
        || matches!(dst, ir::Operand::TargetRegister(_))
        || matches!(src, ir::Operand::Memory { .. })
        || matches!(dst, ir::Operand::Memory { .. })
    {
        validate_ir_copy_assignment(src, dst, strings, label_name, stack)?;
        return emit_ir_copy_instruction(asm, src, dst, strings, label_name, stack);
    }
    validate_ir_copy_assignment(src, dst, strings, label_name, stack)?;
    emit_ir_copy_instruction(asm, src, dst, strings, label_name, stack)
}

fn emit_ir_special_copy(
    asm: &mut String,
    src: &ir::Operand,
    dst: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match src {
        ir::Operand::Pointer(name) => {
            validate_ir_address_copy_dst(dst)?;
            let dst = emit_ir_operand(dst, strings, label_name, stack)?;
            if let Some((offset, _)) = stack_buffer_slot(stack, name) {
                asm::instruction(asm, format_args!("lea {dst}, [rbp - {offset}]"));
            } else {
                asm::instruction(asm, format_args!("lea {dst}, [rip + {name}]"));
            }
        }
        ir::Operand::AddressOf(address) => {
            validate_ir_address_copy_dst(dst)?;
            let dst = emit_ir_operand(dst, strings, label_name, stack)?;
            let address = emit_ir_address(address, stack);
            asm::instruction(asm, format_args!("lea {dst}, [{address}]"));
        }
        ir::Operand::Converted {
            operand,
            conversion,
        } => {
            let ir::Operand::TargetRegister(dst_register) = dst else {
                return Err(String::from(
                    "Width conversion destination must be an integer register",
                ));
            };
            if is_xmm_register(dst_register) {
                return Err(String::from(
                    "Width conversion destination must be an integer register",
                ));
            }
            let dst_width = register_width(dst_register).ok_or_else(|| {
                String::from("Width conversion destination must be an integer register")
            })?;
            let src_width =
                ir_operand_width(operand, strings, label_name, stack).ok_or_else(|| {
                    String::from("Width conversion source must have a known integer width")
                })?;
            if src_width.bits() >= dst_width.bits() {
                return Err(format!(
                    "Width conversion source must be narrower than destination, found {}-bit source and {}-bit destination",
                    src_width.bits(),
                    dst_width.bits()
                ));
            }
            if ir_operand_uses_high_byte(operand) && is_extended_register(dst_register) {
                return Err(String::from(
                    "Width conversion cannot combine high-byte registers ah, bh, ch, or dh with extended registers",
                ));
            }
            let src = emit_ir_operand(operand, strings, label_name, stack)?;
            let dst = emit_ir_operand(dst, strings, label_name, stack)?;
            let opcode = match (conversion, src_width, dst_width) {
                (ir::WidthConversion::ZeroExtend, Width::Bits32, Width::Bits64) => {
                    let dst32 = register_alias(dst_register, Width::Bits32)?;
                    asm::instruction(asm, format_args!("mov {dst32}, {src}"));
                    return Ok(());
                }
                (ir::WidthConversion::ZeroExtend, _, _) => "movzx",
                (ir::WidthConversion::SignExtend, Width::Bits32, Width::Bits64) => "movsxd",
                (ir::WidthConversion::SignExtend, _, _) => "movsx",
            };
            asm::instruction(asm, format_args!("{opcode} {dst}, {src}"));
        }
        ir::Operand::Cast { operand, width } => {
            if width.is_float() {
                let ir::Operand::TargetRegister(dst_register) = dst else {
                    return Err(String::from(
                        "Integer-to-float cast destination must be an XMM register",
                    ));
                };
                if !is_xmm_register(dst_register) {
                    return Err(String::from(
                        "Integer-to-float cast destination must be an XMM register",
                    ));
                }
                let src_width =
                    ir_operand_width(operand, strings, label_name, stack).ok_or_else(|| {
                        String::from("Integer-to-float cast source must have a known width")
                    })?;
                let source_memory_width = match &**operand {
                    ir::Operand::Name(name) => stack_scalar_slot(stack, name)
                        .map(|(_, width)| width)
                        .or_else(|| {
                            strings
                                .integers
                                .get(&(label_name.to_owned(), name.clone()))
                                .and_then(|binding| binding.width)
                        }),
                    _ => ir_operand_memory_width(operand, strings, stack),
                };
                if matches!(
                    source_memory_width,
                    Some(MemoryWidth::U64 | MemoryWidth::Ptr)
                ) {
                    let src = emit_ir_operand(operand, strings, label_name, stack)?;
                    emit_unsigned_u64_to_float_cast(asm, dst_register, &src, *width, operand)?;
                    return Ok(());
                }
                let opcode = match width {
                    MemoryWidth::F32 => "cvtsi2ss",
                    MemoryWidth::F64 => "cvtsi2sd",
                    _ => unreachable!(),
                };
                let unsigned = matches!(src_width, Width::Bits8 | Width::Bits16 | Width::Bits32)
                    && source_memory_width.is_some_and(|width| {
                        matches!(width, MemoryWidth::U8 | MemoryWidth::U16 | MemoryWidth::U32)
                    });
                let src = emit_ir_operand(operand, strings, label_name, stack)?;
                if unsigned {
                    let scratch = if !ir_operand_uses_register_family(operand, "r11") {
                        "r11"
                    } else if !ir_operand_uses_register_family(operand, "r10") {
                        "r10"
                    } else {
                        return Err(String::from(
                            "Unsigned integer-to-float cast source cannot use both r10 and r11",
                        ));
                    };
                    match src_width {
                        Width::Bits8 | Width::Bits16 => {
                            asm::instruction(asm, format_args!("movzx {scratch}, {src}"));
                        }
                        Width::Bits32 => {
                            asm::instruction(asm, format_args!("mov {scratch}d, {src}"));
                        }
                        Width::Bits64 => unreachable!(),
                    }
                    asm::instruction(asm, format_args!("{opcode} {dst_register}, {scratch}"));
                } else {
                    asm::instruction(asm, format_args!("{opcode} {dst_register}, {src}"));
                }
            } else {
                let ir::Operand::TargetRegister(dst_register) = dst else {
                    return Err(String::from(
                        "Float-to-integer cast destination must be an integer register",
                    ));
                };
                if is_xmm_register(dst_register) {
                    return Err(String::from(
                        "Float-to-integer cast destination must be an integer register",
                    ));
                }
                let src_width = ir_operand_memory_width(operand, strings, stack)
                    .or_else(|| match &**operand {
                        ir::Operand::TargetRegister(_) => Some(MemoryWidth::F64),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        String::from("Float-to-integer cast source must have a known width")
                    })?;
                let src = emit_ir_operand(operand, strings, label_name, stack)?;
                emit_float_to_integer_validation_x86(asm, &src, src_width, *width)?;
                let opcode = match src_width {
                    MemoryWidth::F32 => "cvttss2si",
                    MemoryWidth::F64 => "cvttsd2si",
                    _ => {
                        return Err(String::from(
                            "Float-to-integer cast source must be floating-point",
                        ));
                    }
                };
                let dst_width = register_width(dst_register).ok_or_else(|| {
                    String::from("Float-to-integer cast destination must be an integer register")
                })?;
                let unsigned =
                    matches!(width, MemoryWidth::U8 | MemoryWidth::U16 | MemoryWidth::U32);
                if matches!(width, MemoryWidth::U64 | MemoryWidth::Ptr) {
                    emit_float_to_unsigned_u64_cast(asm, dst_register, &src, src_width)?;
                    return Ok(());
                }
                if matches!(dst_width, Width::Bits8 | Width::Bits16) {
                    let scratch = if same_register_family(dst_register, "r11") {
                        "r10"
                    } else {
                        "r11"
                    };
                    let conversion_register = if unsigned {
                        scratch
                    } else {
                        &format!("{scratch}d")
                    };
                    asm::instruction(asm, format_args!("{opcode} {conversion_register}, {src}"));
                    let scratch = register_alias(scratch, dst_width)?;
                    asm::instruction(asm, format_args!("mov {dst_register}, {scratch}"));
                } else if unsigned && matches!(dst_width, Width::Bits32) {
                    let scratch = if same_register_family(dst_register, "r11") {
                        "r10"
                    } else {
                        "r11"
                    };
                    asm::instruction(asm, format_args!("{opcode} {scratch}, {src}"));
                    asm::instruction(asm, format_args!("mov {dst_register}, {scratch}d"));
                } else {
                    asm::instruction(asm, format_args!("{opcode} {dst_register}, {src}"));
                }
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn emit_float_to_integer_validation_x86(
    asm: &mut String,
    src: &str,
    source_width: MemoryWidth,
    destination_width: MemoryWidth,
) -> Result<(), String> {
    let suffix = match source_width {
        MemoryWidth::F32 => "ss",
        MemoryWidth::F64 => "sd",
        _ => {
            return Err(String::from(
                "Float-to-integer cast source must be floating-point",
            ));
        }
    };
    let source_xmm = if src.starts_with("xmm15") {
        "xmm14"
    } else {
        "xmm15"
    };
    let threshold_xmm = if source_xmm == "xmm15" {
        "xmm14"
    } else {
        "xmm15"
    };
    let invalid = format!(".L.__subsea.x86.invalid_cast_{}", asm.len());
    let done = format!(".L.__subsea.x86.valid_cast_{}", asm.len());
    let bits = destination_width.size() * 8;
    let unsigned = matches!(
        destination_width,
        MemoryWidth::U8 | MemoryWidth::U16 | MemoryWidth::U32 | MemoryWidth::U64 | MemoryWidth::Ptr
    );
    asm::push(asm, asm::Operand::Register(String::from("r10")));
    asm::instruction(asm, format_args!("mov{suffix} {source_xmm}, {src}"));
    if unsigned {
        asm::instruction(asm, format_args!("pxor {threshold_xmm}, {threshold_xmm}"));
        asm::instruction(
            asm,
            format_args!("ucomi{suffix} {source_xmm}, {threshold_xmm}"),
        );
        asm::branch(asm, "jp", asm::Operand::Address(invalid.clone()));
        asm::branch(asm, "jb", asm::Operand::Address(invalid.clone()));
    } else {
        let lower = x86_integer_cast_bound(bits, false, false)?;
        load_x86_float_constant(asm, threshold_xmm, source_width, lower);
        asm::instruction(
            asm,
            format_args!("ucomi{suffix} {source_xmm}, {threshold_xmm}"),
        );
        asm::branch(asm, "jp", asm::Operand::Address(invalid.clone()));
        asm::branch(asm, "jb", asm::Operand::Address(invalid.clone()));
    }
    let upper = x86_integer_cast_bound(bits, unsigned, true)?;
    load_x86_float_constant(asm, threshold_xmm, source_width, upper);
    asm::instruction(
        asm,
        format_args!("ucomi{suffix} {source_xmm}, {threshold_xmm}"),
    );
    asm::branch(asm, "jp", asm::Operand::Address(invalid.clone()));
    asm::branch(asm, "jae", asm::Operand::Address(invalid.clone()));
    asm::pop(asm, asm::Operand::Register(String::from("r10")));
    asm::branch(asm, "jmp", asm::Operand::Address(done.clone()));
    asm::label(asm, &invalid);
    asm::pop(asm, asm::Operand::Register(String::from("r10")));
    asm::instruction(asm, "ud2");
    asm::label(asm, &done);
    Ok(())
}

fn x86_integer_cast_bound(
    bits: usize,
    unsigned: bool,
    upper: bool,
) -> Result<&'static str, String> {
    match (bits, unsigned, upper) {
        (8, false, false) => Ok("-128.0"),
        (8, false, true) => Ok("128.0"),
        (16, false, false) => Ok("-32768.0"),
        (16, false, true) => Ok("32768.0"),
        (32, false, false) => Ok("-2147483648.0"),
        (32, false, true) => Ok("2147483648.0"),
        (64, false, false) => Ok("-9223372036854775808.0"),
        (64, false, true) => Ok("9223372036854775808.0"),
        (8, true, true) => Ok("256.0"),
        (16, true, true) => Ok("65536.0"),
        (32, true, true) => Ok("4294967296.0"),
        (64, true, true) => Ok("18446744073709551616.0"),
        _ => Err(String::from("unsupported integer cast bound")),
    }
}

fn load_x86_float_constant(asm: &mut String, register: &str, width: MemoryWidth, value: &str) {
    let immediate = match (width, value) {
        (MemoryWidth::F32, "-128.0") => "0xc3000000",
        (MemoryWidth::F32, "128.0") => "0x43000000",
        (MemoryWidth::F32, "-32768.0") => "0xc7000000",
        (MemoryWidth::F32, "32768.0") => "0x47000000",
        (MemoryWidth::F32, "-2147483648.0") => "0xcf000000",
        (MemoryWidth::F32, "2147483648.0") => "0x4f000000",
        (MemoryWidth::F32, "256.0") => "0x43800000",
        (MemoryWidth::F32, "65536.0") => "0x47800000",
        (MemoryWidth::F32, "4294967296.0") => "0x4f800000",
        (MemoryWidth::F64, "-128.0") => "0xc060000000000000",
        (MemoryWidth::F64, "128.0") => "0x4060000000000000",
        (MemoryWidth::F64, "-32768.0") => "0xc0e0000000000000",
        (MemoryWidth::F64, "32768.0") => "0x40e0000000000000",
        (MemoryWidth::F64, "-2147483648.0") => "0xc1e0000000000000",
        (MemoryWidth::F64, "2147483648.0") => "0x41e0000000000000",
        (MemoryWidth::F64, "256.0") => "0x4070000000000000",
        (MemoryWidth::F64, "65536.0") => "0x40f0000000000000",
        (MemoryWidth::F64, "4294967296.0") => "0x41f0000000000000",
        (MemoryWidth::F64, "-9223372036854775808.0") => "0xc3e0000000000000",
        (MemoryWidth::F64, "9223372036854775808.0") => "0x43e0000000000000",
        (MemoryWidth::F64, "18446744073709551616.0") => "0x43f0000000000000",
        (MemoryWidth::F32, "-9223372036854775808.0") => "0xdf000000",
        (MemoryWidth::F32, "9223372036854775808.0") => "0x5f000000",
        (MemoryWidth::F32, "18446744073709551616.0") => "0x5f800000",
        _ => "0",
    };
    asm::instruction(asm, format_args!("mov r10, {immediate}"));
    if width == MemoryWidth::F32 {
        asm::instruction(asm, format_args!("movd {register}, r10d"));
    } else {
        asm::instruction(asm, format_args!("movq {register}, r10"));
    }
}

fn emit_unsigned_u64_to_float_cast(
    asm: &mut String,
    dst: &str,
    src: &str,
    width: MemoryWidth,
    operand: &ir::Operand,
) -> Result<(), String> {
    let value = match operand {
        ir::Operand::TargetRegister(register) => register.as_str(),
        _ => "r11",
    };
    let work = if same_register_family(value, "r10") {
        "r11"
    } else {
        "r10"
    };
    if !matches!(operand, ir::Operand::TargetRegister(_)) {
        asm::instruction(asm, format_args!("mov {value}, {src}"));
    }
    let constant_xmm = if dst == "xmm15" { "xmm14" } else { "xmm15" };
    let signed_label = format!(".L.__subsea.x86.cast_signed_{}", asm.len());
    let done_label = format!(".L.__subsea.x86.cast_done_{}", asm.len());
    let convert = match width {
        MemoryWidth::F32 => "cvtsi2ss",
        MemoryWidth::F64 => "cvtsi2sd",
        _ => {
            return Err(String::from(
                "Integer-to-float cast destination must be floating-point",
            ));
        }
    };
    let (add, move_constant, constant) = match width {
        MemoryWidth::F32 => ("addss", "movd", "0x3f800000"),
        MemoryWidth::F64 => ("addsd", "movq", "0x3ff0000000000000"),
        _ => unreachable!(),
    };
    asm::instruction(asm, format_args!("test {value}, {value}"));
    asm::branch(asm, "jns", asm::Operand::Address(signed_label.clone()));
    asm::instruction(asm, format_args!("mov {work}, {value}"));
    asm::instruction(asm, format_args!("shr {work}, 1"));
    asm::instruction(asm, format_args!("{convert} {dst}, {work}"));
    asm::instruction(asm, format_args!("{add} {dst}, {dst}"));
    asm::instruction(asm, format_args!("test {value}, 1"));
    asm::branch(asm, "jz", asm::Operand::Address(done_label.clone()));
    asm::instruction(asm, format_args!("mov {work}, {constant}"));
    asm::instruction(asm, format_args!("{move_constant} {constant_xmm}, {work}"));
    asm::instruction(asm, format_args!("{add} {dst}, {constant_xmm}"));
    asm::branch(asm, "jmp", asm::Operand::Address(done_label.clone()));
    asm::label(asm, &signed_label);
    asm::instruction(asm, format_args!("{convert} {dst}, {value}"));
    asm::label(asm, &done_label);
    Ok(())
}

fn emit_float_to_unsigned_u64_cast(
    asm: &mut String,
    dst: &str,
    src: &str,
    width: MemoryWidth,
) -> Result<(), String> {
    let source_xmm = if src.starts_with("xmm15") {
        "xmm14"
    } else {
        "xmm15"
    };
    let threshold_xmm = if source_xmm == "xmm15" {
        "xmm14"
    } else {
        "xmm15"
    };
    let scratch = if same_register_family(dst, "r10") {
        "r11"
    } else {
        "r10"
    };
    let (move_source, move_constant, compare, subtract, convert, threshold, high_bit) = match width
    {
        MemoryWidth::F32 => (
            "movss",
            "movd",
            "ucomiss",
            "subss",
            "cvttss2si",
            "0x5f000000",
            "0x8000000000000000",
        ),
        MemoryWidth::F64 => (
            "movsd",
            "movq",
            "ucomisd",
            "subsd",
            "cvttsd2si",
            "0x43e0000000000000",
            "0x8000000000000000",
        ),
        _ => {
            return Err(String::from(
                "Float-to-integer cast source must be floating-point",
            ));
        }
    };
    let signed_label = format!(".L.__subsea.x86.cast_signed_{}", asm.len());
    let done_label = format!(".L.__subsea.x86.cast_done_{}", asm.len());
    asm::instruction(asm, format_args!("{move_source} {source_xmm}, {src}"));
    asm::instruction(asm, format_args!("mov {scratch}, {threshold}"));
    asm::instruction(
        asm,
        format_args!("{move_constant} {threshold_xmm}, {scratch}"),
    );
    asm::instruction(asm, format_args!("{compare} {source_xmm}, {threshold_xmm}"));
    asm::branch(asm, "jb", asm::Operand::Address(signed_label.clone()));
    asm::instruction(
        asm,
        format_args!("{subtract} {source_xmm}, {threshold_xmm}"),
    );
    asm::instruction(asm, format_args!("{convert} {dst}, {source_xmm}"));
    asm::instruction(asm, format_args!("mov {scratch}, {high_bit}"));
    asm::instruction(asm, format_args!("or {dst}, {scratch}"));
    asm::branch(asm, "jmp", asm::Operand::Address(done_label.clone()));
    asm::label(asm, &signed_label);
    asm::instruction(asm, format_args!("{convert} {dst}, {source_xmm}"));
    asm::label(asm, &done_label);
    Ok(())
}

fn validate_ir_address_copy_dst(dst: &ir::Operand) -> Result<(), String> {
    let ir::Operand::TargetRegister(register) = dst else {
        return Err(String::from(
            "Address-of labels can only be copied into registers for now",
        ));
    };
    if is_xmm_register(register) {
        return Err(String::from(
            "Address-of labels can only be copied into 64-bit integer registers",
        ));
    }
    match register_width(register) {
        Some(Width::Bits64) => Ok(()),
        Some(width) => Err(format!(
            "Address-of labels can only be copied into 64-bit registers, found {}-bit register",
            width.bits()
        )),
        None => Err(String::from(
            "Address-of labels can only be copied into 64-bit registers",
        )),
    }
}

fn emit_ir_binary_value_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    op: MathOp,
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if op == MathOp::Power {
        return emit_ir_power_assignment(asm, dst, lhs, rhs, strings, label_name, stack);
    }
    if integer_op_can_be_float(op)
        && (ir_binary_may_be_float(lhs, rhs)
            || ir_operand_is_float(lhs, strings, label_name, stack)
            || ir_operand_is_float(rhs, strings, label_name, stack))
    {
        let width = match resolve_ir_float_binary_width(lhs, rhs, strings, label_name, stack) {
            Ok(Some(width)) => width,
            Ok(None) => unreachable!(),
            Err(_) => {
                return Err(format!(
                    "Floating-point arithmetic width is ambiguous; use f32{} or f64{}",
                    math_op_symbol(op),
                    math_op_symbol(op)
                ));
            }
        };
        return emit_ir_float_binary_assignment(
            asm,
            dst,
            width,
            float_math_op_from_integer_op(op),
            lhs,
            rhs,
            strings,
            label_name,
            stack,
        )
        .map_err(|error| {
            if error == "Floating-point arithmetic width is ambiguous; use f32 or f64" {
                format!(
                    "Floating-point arithmetic width is ambiguous; use f32{} or f64{}",
                    math_op_symbol(op),
                    math_op_symbol(op)
                )
            } else {
                error
            }
        });
    }
    validate_ir_integer_binary_assignment(dst, op, lhs, rhs, strings, label_name, stack)?;
    emit_ir_integer_binary_assignment(asm, dst, op, lhs, rhs, strings, label_name, stack)
}

fn emit_ir_bitwise_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    op: BitwiseUnaryOp,
    src: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if ir_operand_is_float(dst, strings, label_name, stack)
        || ir_operand_is_float(src, strings, label_name, stack)
    {
        return Err(format!(
            "{} does not support floating-point operands yet",
            bitwise_unary_opcode(op)
        ));
    }
    validate_ir_copy_assignment(src, dst, strings, label_name, stack)?;
    emit_ir_copy_instruction(asm, src, dst, strings, label_name, stack)?;
    let dst = emit_ir_operand(dst, strings, label_name, stack)?;
    asm::instruction(asm, format_args!("{} {}", bitwise_unary_opcode(op), dst));
    Ok(())
}

fn ir_float_width(
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<MemoryWidth, String> {
    let width =
        [lhs, rhs].into_iter().find_map(|operand| match operand {
            ir::Operand::FloatLiteral(value) => strings
                .float_literals
                .iter()
                .find(|((label, _, literal), _)| label == label_name && literal == value)
                .map(|((_, width, _), _)| *width),
            ir::Operand::Name(name) => stack_scalar_slot(stack, name)
                .map(|(_, width)| width)
                .filter(|width| width.is_float())
                .or_else(|| {
                    strings
                        .float_bindings
                        .get(&(label_name.to_owned(), name.clone()))
                        .map(|binding| binding.width)
                }),
            ir::Operand::Memory { address, width } => width
                .filter(|width| width.is_float())
                .or_else(|| match &address.first {
                    ir::AddressTerm::Name(name) => strings
                        .memory_widths
                        .get(name)
                        .copied()
                        .filter(|width| width.is_float()),
                    _ => None,
                }),
            _ => None,
        });
    width.ok_or_else(|| {
        let _ = stack;
        String::from("Floating-point arithmetic width is ambiguous; use f32 or f64")
    })
}

fn resolve_ir_float_binary_width(
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<Option<MemoryWidth>, String> {
    if !ir_binary_may_be_float(lhs, rhs)
        && !ir_operand_is_float(lhs, strings, label_name, stack)
        && !ir_operand_is_float(rhs, strings, label_name, stack)
    {
        return Ok(None);
    }
    Ok(Some(ir_float_width(lhs, rhs, strings, label_name, stack)?))
}

fn emit_ir_expression_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    op: ExprOp,
    lhs: &ir::Value,
    rhs: &ir::Value,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_ir_expression(lhs, strings, label_name, stack)?;
    validate_ir_expression(rhs, strings, label_name, stack)?;
    let target = match dst {
        ir::Operand::TargetRegister(name) if !is_xmm_register(name) => dst.clone(),
        _ => ir::Operand::TargetRegister(ir_expression_temp_register(dst, lhs, rhs)?),
    };
    emit_ir_expression_to_register(asm, &target, op, lhs, rhs, strings, label_name, stack)?;
    if target != *dst {
        emit_ir_copy_instruction(asm, &target, dst, strings, label_name, stack)?;
    }
    Ok(())
}

fn emit_ir_expression_to_register(
    asm: &mut String,
    dst: &ir::Operand,
    op: ExprOp,
    lhs: &ir::Value,
    rhs: &ir::Value,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let ir::Operand::TargetRegister(register) = dst else {
        return Err(String::from(
            "Expression destination must be an integer register",
        ));
    };
    if is_xmm_register(register) {
        return Err(String::from(
            "Expression destination must be an integer register",
        ));
    }

    if matches!(op, ExprOp::Power | ExprOp::Math(MathOp::Power)) {
        let ir::Value::Operand(base) = lhs else {
            return Err(String::from("Power base must be an operand"));
        };
        let ir::Value::Operand(exponent) = rhs else {
            return Err(String::from("Power exponent must be an operand"));
        };
        return emit_ir_power_operation(asm, dst, base, exponent, strings, label_name, stack);
    }

    let rhs_temp = if ir_value_uses_register_family(rhs, register) {
        let temp = ir::Operand::TargetRegister(ir_expression_temp_register(dst, lhs, rhs)?);
        emit_ir_expression_value(asm, &temp, rhs, strings, label_name, stack)?;
        Some(temp)
    } else {
        None
    };
    emit_ir_expression_value(asm, dst, lhs, strings, label_name, stack)?;
    let rhs_operand = if let Some(temp) = rhs_temp {
        temp
    } else if let ir::Value::Operand(operand) = rhs {
        operand.clone()
    } else {
        let temp = ir::Operand::TargetRegister(ir_expression_temp_register(dst, lhs, rhs)?);
        emit_ir_expression_value(asm, &temp, rhs, strings, label_name, stack)?;
        temp
    };

    match op {
        ExprOp::Math(op) => {
            emit_ir_integer_math_instruction(asm, op, &rhs_operand, dst, strings, label_name, stack)
        }
        ExprOp::Divide { signed } => emit_ir_division_from_accumulator(
            asm,
            signed,
            false,
            lhs,
            &rhs_operand,
            dst,
            strings,
            label_name,
            stack,
        ),
        ExprOp::Modulo { signed } => emit_ir_division_from_accumulator(
            asm,
            signed,
            true,
            lhs,
            &rhs_operand,
            dst,
            strings,
            label_name,
            stack,
        ),
        ExprOp::Power => unreachable!(),
    }
}

fn emit_ir_expression_value(
    asm: &mut String,
    dst: &ir::Operand,
    value: &ir::Value,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match value {
        ir::Value::Operand(operand) => {
            validate_ir_copy_assignment(operand, dst, strings, label_name, stack)?;
            emit_ir_copy_instruction(asm, operand, dst, strings, label_name, stack)
        }
        ir::Value::Expression { op, lhs, rhs } => {
            emit_ir_expression_to_register(asm, dst, *op, lhs, rhs, strings, label_name, stack)
        }
        _ => Err(String::from("Arithmetic expressions must contain operands")),
    }
}

fn validate_ir_expression(
    value: &ir::Value,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match value {
        ir::Value::Operand(operand) => {
            if ir_operand_is_float(operand, strings, label_name, stack) {
                return Err(String::from(
                    "Arithmetic expressions do not support floating-point operands yet",
                ));
            }
            if matches!(operand, ir::Operand::FloatLiteral(_)) {
                return Err(String::from(
                    "Arithmetic expressions do not support floating-point operands yet",
                ));
            }
            if matches!(operand, ir::Operand::Pointer(_) | ir::Operand::AddressOf(_)) {
                return Err(String::from(
                    "Arithmetic expressions cannot use address-of operands",
                ));
            }
            Ok(())
        }
        ir::Value::Expression { lhs, rhs, .. } => {
            validate_ir_expression(lhs, strings, label_name, stack)?;
            validate_ir_expression(rhs, strings, label_name, stack)
        }
        _ => Err(String::from("Arithmetic expressions must contain operands")),
    }
}

fn ir_value_uses_register_family(value: &ir::Value, register: &str) -> bool {
    match value {
        ir::Value::Operand(operand) => ir_operand_uses_register_family(operand, register),
        ir::Value::Expression { lhs, rhs, .. } => {
            ir_value_uses_register_family(lhs, register)
                || ir_value_uses_register_family(rhs, register)
        }
        _ => false,
    }
}

fn ir_expression_temp_register(
    dst: &ir::Operand,
    lhs: &ir::Value,
    rhs: &ir::Value,
) -> Result<String, String> {
    for register in ["r10", "r11", "r8", "r9"] {
        if !ir_operand_uses_register_family(dst, register)
            && !ir_value_uses_register_family(lhs, register)
            && !ir_value_uses_register_family(rhs, register)
        {
            return Ok(register.to_owned());
        }
    }
    Err(String::from(
        "Arithmetic expression has no available temporary register",
    ))
}

fn emit_ir_division_from_accumulator(
    asm: &mut String,
    signed: bool,
    remainder: bool,
    lhs: &ir::Value,
    rhs: &ir::Operand,
    dst: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let ir::Value::Operand(lhs) = lhs else {
        return Err(String::from("Division left operand must be an operand"));
    };
    validate_ir_copy_assignment(
        lhs,
        &ir::Operand::TargetRegister(String::from("rax")),
        strings,
        label_name,
        stack,
    )?;
    let divisor = if matches!(rhs, ir::Operand::Immediate(_))
        || ir_operand_uses_register_family(rhs, "rax")
        || ir_operand_uses_register_family(rhs, "rdx")
    {
        let temp = ir_expression_temp_register(
            &ir::Operand::TargetRegister(String::from("rax")),
            &ir::Value::Operand(lhs.clone()),
            &ir::Value::Operand(rhs.clone()),
        )?;
        let temp = ir::Operand::TargetRegister(temp);
        emit_ir_copy_instruction(asm, rhs, &temp, strings, label_name, stack)?;
        temp
    } else {
        rhs.clone()
    };
    emit_ir_copy_instruction(
        asm,
        lhs,
        &ir::Operand::TargetRegister(String::from("rax")),
        strings,
        label_name,
        stack,
    )?;
    asm::prepare_division(asm, signed);
    let divisor = emit_ir_operand(&divisor, strings, label_name, stack)?;
    asm::divide(
        asm,
        division_opcode(signed).to_owned(),
        asm::Operand::Address(divisor),
    );
    let result = ir::Operand::TargetRegister(if remainder { "rdx" } else { "rax" }.to_owned());
    if result != *dst {
        emit_ir_copy_instruction(asm, &result, dst, strings, label_name, stack)?;
    }
    Ok(())
}

fn emit_ir_power_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    base: &ir::Operand,
    exponent: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    emit_ir_power_operation(asm, dst, base, exponent, strings, label_name, stack)
}

fn emit_ir_power_operation(
    asm: &mut String,
    dst: &ir::Operand,
    base: &ir::Operand,
    exponent: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let ir::Operand::TargetRegister(register) = dst else {
        return Err(String::from("Power destination must be a register for now"));
    };
    if is_xmm_register(register) {
        return Err(String::from(
            "Power destination must be an integer register",
        ));
    }
    if register_width(register) != Some(Width::Bits64) {
        return Err(String::from(
            "Power destination must be a 64-bit integer register",
        ));
    }
    if ir_operand_is_float(base, strings, label_name, stack)
        || ir_operand_is_float(exponent, strings, label_name, stack)
    {
        return Err(String::from("Power exponent must be an integer operand"));
    }
    if matches!(base, ir::Operand::Pointer(_) | ir::Operand::AddressOf(_)) {
        return Err(String::from("Power base cannot be an address-of operand"));
    }
    if matches!(
        exponent,
        ir::Operand::Pointer(_) | ir::Operand::AddressOf(_)
    ) {
        return Err(String::from(
            "Power exponent cannot be an address-of operand",
        ));
    }
    if ir_operand_immediate_value(exponent, strings, label_name).is_some_and(|value| value < 0) {
        return Err(String::from("Power exponent must be non-negative"));
    }
    if ir_operand_uses_register_family(dst, "r10") || ir_operand_uses_register_family(dst, "r11") {
        return Err(String::from(
            "Power destination cannot use r10 or r11 because they are scratch registers",
        ));
    }
    let exponent_r10 = ir_operand_uses_register_family(exponent, "r10");
    let base_r11 = matches!(base, ir::Operand::Memory { address, .. } if ir_address_uses_register_family(address, &|name| same_register_family(name, "r11")));
    if exponent_r10 && base_r11 {
        return Err(String::from(
            "Power cannot use r10 in the exponent and r11 in the base address because both are scratch registers",
        ));
    }
    if exponent_r10 {
        emit_ir_power_exponent_load(asm, exponent, strings, label_name, stack)?;
        emit_ir_copy_instruction(
            asm,
            base,
            &ir::Operand::TargetRegister("r10".to_owned()),
            strings,
            label_name,
            stack,
        )?;
    } else {
        emit_ir_copy_instruction(
            asm,
            base,
            &ir::Operand::TargetRegister("r10".to_owned()),
            strings,
            label_name,
            stack,
        )?;
        emit_ir_power_exponent_load(asm, exponent, strings, label_name, stack)?;
    }
    let loop_label = format!(".L.__subsea.{label_name}.pow_{}_loop", asm.len());
    let skip_label = format!(".L.__subsea.{label_name}.pow_{}_skip_mul", asm.len());
    let done_label = format!(".L.__subsea.{label_name}.pow_{}_done", asm.len());
    asm::instruction(asm, format_args!("mov {register}, 1"));
    asm::label(asm, &loop_label);
    asm::instruction(asm, "test r11, r11");
    asm::branch(asm, "je", asm::Operand::Address(done_label.clone()));
    asm::instruction(asm, "test r11, 1");
    asm::branch(asm, "je", asm::Operand::Address(skip_label.clone()));
    asm::instruction(asm, format_args!("imul {register}, r10"));
    asm::label(asm, &skip_label);
    asm::instruction(asm, "imul r10, r10");
    asm::instruction(asm, "shr r11, 1");
    asm::jump(asm, asm::Operand::Address(loop_label));
    asm::label(asm, &done_label);
    Ok(())
}

fn emit_ir_power_exponent_load(
    asm: &mut String,
    exponent: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if let Some(value) = ir_operand_immediate_value(exponent, strings, label_name) {
        validate_immediate_range(value, ImmediateDestination::Register(Width::Bits64))?;
        asm::instruction(asm, format_args!("mov r11, {value}"));
        return Ok(());
    }
    if ir_operand_uses_high_byte(exponent) {
        return Err(String::from(
            "Power exponent cannot use high-byte registers ah, bh, ch, or dh",
        ));
    }
    let width = ir_operand_width(exponent, strings, label_name, stack)
        .ok_or_else(|| String::from("Power exponent must be an integer operand"))?;
    let exponent = emit_ir_operand(exponent, strings, label_name, stack)?;
    match width {
        Width::Bits64 => asm::instruction(asm, format_args!("mov r11, {exponent}")),
        Width::Bits32 => asm::instruction(asm, format_args!("mov r11d, {exponent}")),
        Width::Bits16 | Width::Bits8 => {
            asm::instruction(asm, format_args!("movzx r11, {exponent}"))
        }
    }
    Ok(())
}

fn emit_ir_string_bytes_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    value: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let ir::Operand::Memory { address, width } = dst else {
        return Err(String::from(
            "String byte assignment destination must be a memory operand",
        ));
    };

    if width.is_some() {
        return Err(String::from(
            "String byte assignment destination cannot specify a memory width",
        ));
    }

    let base = emit_ir_address(address, stack);
    for (index, byte) in value.bytes().enumerate() {
        let address = if index == 0 {
            base.clone()
        } else {
            format!("{base} + {index}")
        };
        asm::instruction(asm, format_args!("mov byte ptr [{address}], {byte}"));
    }

    Ok(())
}

fn emit_ir_platform_reserve_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    len: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if let Some(value) = ir_operand_immediate_value(len, strings, label_name) {
        validate_immediate_range(value, ImmediateDestination::Memory(MemoryWidth::U64))?;
        asm::mov(
            asm,
            asm::Operand::Register(String::from("rsi")),
            asm::Operand::Immediate(value),
        );
    } else {
        match ir_operand_width(len, strings, label_name, stack) {
            Some(Width::Bits64) => {
                let len = emit_ir_operand(len, strings, label_name, stack)?;
                asm::mov(
                    asm,
                    asm::Operand::Register(String::from("rsi")),
                    asm::Operand::Address(len),
                );
            }
            Some(width) => {
                return Err(format!(
                    "reserve size must be 64-bit, found {}-bit operand",
                    width.bits()
                ));
            }
            None => {
                return Err(String::from(
                    "reserve size must be an integer immediate, const, 64-bit register, or 64-bit stack variable",
                ));
            }
        }
    }

    asm::mov(
        asm,
        asm::Operand::Register(String::from("rdi")),
        asm::Operand::Immediate(linux::STDIN as i128),
    );
    asm::mov(
        asm,
        asm::Operand::Register(String::from("rdx")),
        asm::Operand::Immediate(3),
    );
    asm::mov(
        asm,
        asm::Operand::Register(String::from("r10")),
        asm::Operand::Immediate(34),
    );
    asm::mov(
        asm,
        asm::Operand::Register(String::from("r8")),
        asm::Operand::Immediate(-1),
    );
    asm::mov(
        asm,
        asm::Operand::Register(String::from("r9")),
        asm::Operand::Immediate(0),
    );
    emit_linux_mmap(asm);

    if dst != &ir::Operand::TargetRegister(String::from("rax")) {
        emit_ir_copy_instruction(
            asm,
            &ir::Operand::TargetRegister(String::from("rax")),
            dst,
            strings,
            label_name,
            stack,
        )?;
    }

    Ok(())
}

fn validate_ir_copy_assignment(
    src: &ir::Operand,
    dst: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(
        dst,
        ir::Operand::Immediate(_)
            | ir::Operand::Pointer(_)
            | ir::Operand::AddressOf(_)
            | ir::Operand::StringProperty { .. }
            | ir::Operand::Converted { .. }
            | ir::Operand::Cast { .. }
    ) || matches!(dst, ir::Operand::Name(name) if stack_scalar_slot(stack, name).is_none())
    {
        return Err(String::from(
            "mov destination must be a register or memory operand",
        ));
    }

    if matches!(src, ir::Operand::Pointer(_) | ir::Operand::AddressOf(_)) {
        return Err(String::from("mov source cannot be an address-of operand"));
    }
    if matches!(
        src,
        ir::Operand::Converted { .. } | ir::Operand::Cast { .. }
    ) {
        return Err(String::from("mov source cannot use conversion here"));
    }
    if matches!(src, ir::Operand::FloatLiteral(_)) || matches!(dst, ir::Operand::FloatLiteral(_)) {
        return Err(String::from(
            "mov cannot use floating-point literal operands",
        ));
    }
    if ir_operand_is_memory(src, stack) && ir_operand_is_memory(dst, stack) {
        return Err(String::from(
            "mov cannot use memory for both source and destination",
        ));
    }
    if ir_operand_is_float(src, strings, label_name, stack)
        != ir_operand_is_float(dst, strings, label_name, stack)
    {
        return Err(if ir_operand_is_float(src, strings, label_name, stack) {
            String::from(
                "Floating-point memory operands require an XMM register source or destination",
            )
        } else {
            String::from(
                "XMM moves require one XMM register and one explicitly f32 or f64 memory operand",
            )
        });
    }
    if ir_operand_uses_high_byte(src) && ir_operand_uses_extended(dst) {
        return Err(String::from(
            "mov cannot combine high-byte registers ah, bh, ch, or dh with extended registers",
        ));
    }
    if ir_operand_uses_high_byte(dst) && ir_operand_uses_extended(src) {
        return Err(String::from(
            "mov cannot combine high-byte registers ah, bh, ch, or dh with extended registers",
        ));
    }

    if let (Some(src_width), Some(dst_width)) = (
        ir_operand_width(src, strings, label_name, stack),
        ir_operand_width(dst, strings, label_name, stack),
    ) && src_width != dst_width
    {
        if matches!(src, ir::Operand::TargetRegister(_))
            && matches!(dst, ir::Operand::TargetRegister(_))
        {
            if src_width.bits() < dst_width.bits() {
                return Err(format!(
                    "Cannot use {}-bit source with {}-bit destination",
                    src_width.bits(),
                    dst_width.bits()
                ));
            }
        } else if src_width.bits() > dst_width.bits()
            && matches!(src, ir::Operand::TargetRegister(_))
        {
            // Register and explicit-memory destinations use the source's
            // narrow alias for the same truncating move as the AST backend.
        } else {
            return Err(format!(
                "Cannot use {}-bit source with {}-bit destination",
                src_width.bits(),
                dst_width.bits()
            ));
        }
    }
    if let Some(value) = ir_operand_immediate_value(src, strings, label_name) {
        if matches!(dst, ir::Operand::Memory { width: None, .. })
            && ir_operand_memory_width(dst, strings, stack).is_none()
        {
            return Err(String::from(
                "Cannot assign an immediate value directly into memory without an explicit width",
            ));
        }
        if let Some(width) = ir_operand_memory_width(dst, strings, stack) {
            validate_immediate_range(value, ImmediateDestination::Memory(width))?;
        } else if let Some(width) = ir_operand_width(dst, strings, label_name, stack) {
            validate_immediate_range(value, ImmediateDestination::Register(width))?;
        }
    }

    // Force the same binding/address diagnostics as the IR emitter before it
    // produces assembly, without rebuilding an AST operand.
    let _ = ir_machine_operand(src, strings, label_name, stack)?;
    let _ = ir_machine_operand(dst, strings, label_name, stack)?;
    Ok(())
}

fn validate_ir_integer_binary_assignment(
    dst: &ir::Operand,
    op: MathOp,
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if is_shift_math_op(op)
        && !matches!(rhs, ir::Operand::Immediate(_))
        && !matches!(rhs, ir::Operand::TargetRegister(name) if name == "cl")
    {
        return Err(format!(
            "{} count must be an immediate value or cl, found {}",
            integer_math_opcode(op),
            match rhs {
                ir::Operand::TargetRegister(name) => format!("register {name}"),
                _ => String::from("operand"),
            }
        ));
    }
    if lhs == dst {
        return if is_shift_math_op(op) {
            let _ = ir_machine_operand(rhs, strings, label_name, stack)?;
            Ok(())
        } else {
            validate_ir_copy_assignment(rhs, dst, strings, label_name, stack)
        };
    }
    if rhs == dst {
        if is_commutative_math_op(op) {
            return validate_ir_copy_assignment(lhs, dst, strings, label_name, stack);
        }
        if op == MathOp::Subtract {
            return validate_ir_copy_assignment(lhs, dst, strings, label_name, stack);
        }
        return Err(format!(
            "Binary assignment destination cannot also be the right operand for {}",
            math_op_symbol(op)
        ));
    }

    if dst != lhs && dst != rhs && ir_operand_address_uses_register_family(rhs, dst) {
        let name = match dst {
            ir::Operand::TargetRegister(name) => name,
            _ => "destination",
        };
        return Err(format!(
            "Binary assignment destination {name} cannot be used in the right operand address"
        ));
    }

    validate_ir_copy_assignment(lhs, dst, strings, label_name, stack)?;
    if is_shift_math_op(op) {
        let _ = ir_machine_operand(rhs, strings, label_name, stack)?;
        Ok(())
    } else {
        validate_ir_copy_assignment(rhs, dst, strings, label_name, stack)
    }
}

fn ir_operand_is_memory(operand: &ir::Operand, stack: &StackFrame) -> bool {
    matches!(operand, ir::Operand::Memory { .. })
        || matches!(operand, ir::Operand::Name(name) if stack_scalar_slot(stack, name).is_some())
}

fn ir_operand_width(
    operand: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Option<Width> {
    match operand {
        ir::Operand::TargetRegister(name) => register_width(name),
        ir::Operand::Name(name) => stack_scalar_slot(stack, name)
            .map(|(_, width)| memory_width_bits(width))
            .or_else(|| {
                strings
                    .integers
                    .get(&(label_name.to_owned(), name.clone()))
                    .and_then(|binding| binding.width.map(memory_width_bits))
            }),
        ir::Operand::Memory { address, width } => {
            width
                .map(memory_width_bits)
                .or_else(|| match &address.first {
                    ir::AddressTerm::Name(name) => strings
                        .memory_widths
                        .get(name)
                        .map(|width| memory_width_bits(*width)),
                    _ => None,
                })
        }
        ir::Operand::StringProperty { .. } => Some(Width::Bits64),
        _ => None,
    }
}

fn ir_operand_address_uses_register_family(operand: &ir::Operand, dst: &ir::Operand) -> bool {
    let ir::Operand::TargetRegister(dst) = dst else {
        return false;
    };
    match operand {
        ir::Operand::Memory { address, .. } | ir::Operand::AddressOf(address) => {
            ir_address_uses_register_family(address, &|register| {
                same_register_family(register, dst)
            })
        }
        _ => false,
    }
}

fn ir_operand_memory_width(
    operand: &ir::Operand,
    strings: &StringTable,
    stack: &StackFrame,
) -> Option<MemoryWidth> {
    match operand {
        ir::Operand::Memory {
            width: Some(width), ..
        } => Some(*width),
        ir::Operand::Memory {
            address,
            width: None,
        } => match &address.first {
            ir::AddressTerm::Name(name) => strings.memory_widths.get(name).copied(),
            _ => None,
        },
        ir::Operand::Name(name) => stack_scalar_slot(stack, name).map(|(_, width)| width),
        _ => None,
    }
}

fn ir_operand_immediate_value(
    operand: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
) -> Option<i128> {
    match operand {
        ir::Operand::Immediate(value) => Some(*value),
        ir::Operand::Name(name) => strings
            .integers
            .get(&(label_name.to_owned(), name.clone()))
            .map(|binding| binding.value),
        _ => None,
    }
}

fn ir_operand_is_float(
    operand: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> bool {
    match operand {
        ir::Operand::TargetRegister(name) => is_xmm_register(name),
        ir::Operand::FloatLiteral(_) => true,
        ir::Operand::Memory { address, width } => {
            width.is_some_and(MemoryWidth::is_float)
                || matches!(&address.first, ir::AddressTerm::Name(name)
                if strings
                    .memory_widths
                    .get(name)
                    .is_some_and(|width| width.is_float()))
        }
        ir::Operand::Name(name) => {
            stack_scalar_slot(stack, name).is_some_and(|(_, width)| width.is_float())
                || strings
                    .float_bindings
                    .contains_key(&(label_name.to_owned(), name.clone()))
        }
        _ => false,
    }
}

fn ir_operand_uses_high_byte(operand: &ir::Operand) -> bool {
    matches!(operand, ir::Operand::TargetRegister(name) if is_high_byte_register(name))
        || matches!(operand, ir::Operand::Memory { address, .. } if ir_address_uses_register(address, is_high_byte_register))
}

fn ir_operand_uses_extended(operand: &ir::Operand) -> bool {
    matches!(operand, ir::Operand::TargetRegister(name) if is_extended_register(name))
        || matches!(operand, ir::Operand::Memory { address, .. } if ir_address_uses_register(address, is_extended_register))
}

fn ir_address_uses_register(address: &ir::Address, predicate: fn(&str) -> bool) -> bool {
    let uses_register = |term: &ir::AddressTerm| match term {
        ir::AddressTerm::TargetRegister(name)
        | ir::AddressTerm::ScaledTargetRegister { register: name, .. } => predicate(name),
        _ => false,
    };
    uses_register(&address.first) || address.rest.iter().any(|(_, term)| uses_register(term))
}

fn emit_ir_float_binary_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    width: MemoryWidth,
    op: FloatMathOp,
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_float_width(width)?;

    let ir::Operand::TargetRegister(dst_register) = dst else {
        return Err(String::from(
            "Floating-point arithmetic destination must be an XMM register",
        ));
    };
    if !is_xmm_register(dst_register) {
        return Err(String::from(
            "Floating-point arithmetic destination must be an XMM register",
        ));
    }

    validate_ir_float_math_operand(
        "Floating-point arithmetic left operand",
        lhs,
        width,
        strings,
        label_name,
        stack,
    )?;
    validate_ir_float_math_operand(
        "Floating-point arithmetic right operand",
        rhs,
        width,
        strings,
        label_name,
        stack,
    )?;

    if lhs != dst {
        let lhs = emit_ir_float_operand(lhs, width, strings, label_name, stack)?;
        asm::float_move(
            asm,
            x86_float_move_opcode(width)?.to_owned(),
            asm::Operand::Register(dst_register.clone()),
            asm::Operand::Address(lhs),
        );
    }

    if rhs == dst && !matches!(op, FloatMathOp::Add | FloatMathOp::Multiply) {
        return Err(String::from(
            "Non-commutative floating-point assignment destination cannot also be the right operand",
        ));
    }

    let rhs = if rhs == dst {
        emit_ir_float_operand(lhs, width, strings, label_name, stack)?
    } else {
        emit_ir_float_operand(rhs, width, strings, label_name, stack)?
    };
    asm::float_binary(
        asm,
        float_math_opcode(op, width).to_owned(),
        asm::Operand::Register(dst_register.clone()),
        asm::Operand::Address(rhs),
    );
    Ok(())
}

fn validate_ir_float_math_operand(
    name: &str,
    operand: &ir::Operand,
    width: MemoryWidth,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match operand {
        ir::Operand::Converted { .. } | ir::Operand::Cast { .. } => Err(format!(
            "{name} cannot use integer width conversion in floating-point math"
        )),
        ir::Operand::AddressOf(_) => Err(format!("{name} cannot be an address-of operand")),
        ir::Operand::TargetRegister(register) if is_xmm_register(register) => Ok(()),
        ir::Operand::FloatLiteral(value) => validate_float_literal(value, width),
        ir::Operand::Name(binding) if stack_scalar_slot(stack, binding).is_some() => {
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
        ir::Operand::Name(binding) => match strings
            .float_bindings
            .get(&(label_name.to_owned(), binding.clone()))
        {
            Some(float) if float.width == width => Ok(()),
            Some(_) => Err(format!(
                "{name} width must match the floating-point operator width"
            )),
            None => Err(format!("{name} cannot be a const or stack binding for now")),
        },
        ir::Operand::Memory {
            address,
            width: memory_width,
        } => match ir_operand_memory_width(operand, strings, stack) {
            Some(resolved_width) if resolved_width == width => Ok(()),
            Some(MemoryWidth::F32 | MemoryWidth::F64) => Err(format!(
                "{name} width must match the floating-point operator width"
            )),
            Some(_) => Err(format!(
                "{name} must be an XMM register or floating-point memory operand"
            )),
            None => {
                let _ = address;
                let _ = memory_width;
                Err(format!(
                    "{name} memory operand requires an explicit f32 or f64 width"
                ))
            }
        },
        ir::Operand::Immediate(_) => Err(format!(
            "{name} cannot be an immediate value; use a floating-point memory operand for now"
        )),
        ir::Operand::StringProperty { .. } => Err(format!("{name} cannot be a string property")),
        ir::Operand::Pointer(_) => Err(format!("{name} cannot be an address-of operand")),
        ir::Operand::TargetRegister(register) => Err(format!(
            "{name} must be an XMM register, found integer register {register}"
        )),
    }
}

fn validate_ir_float_intrinsic_destination(
    name: &str,
    dst: &ir::Operand,
    width: MemoryWidth,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<bool, String> {
    match dst {
        ir::Operand::TargetRegister(register) if is_xmm_register(register) => Ok(false),
        dst if ir_operand_is_memory(dst, stack)
            && !matches!(dst, ir::Operand::StringProperty { .. })
            && ir_operand_is_float(dst, strings, label_name, stack) =>
        {
            if ir_operand_memory_width(dst, strings, stack) != Some(width) {
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

fn emit_ir_float_operand(
    operand: &ir::Operand,
    width: MemoryWidth,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<String, String> {
    match operand {
        ir::Operand::FloatLiteral(value) => {
            let binding = strings
                .float_literals
                .get(&(label_name.to_owned(), width, value.clone()))
                .ok_or_else(|| String::from("Internal error: missing float literal"))?;
            Ok(format!(
                "{} ptr [rip + {}]",
                binding.width.ptr(),
                binding.asm_label
            ))
        }
        ir::Operand::Name(name) if stack_scalar_slot(stack, name).is_none() => {
            let binding = strings
                .float_bindings
                .get(&(label_name.to_owned(), name.clone()))
                .ok_or_else(|| format!("Unknown float binding {name:?} in label {label_name:?}"))?;
            Ok(format!(
                "{} ptr [rip + {}]",
                binding.width.ptr(),
                binding.asm_label
            ))
        }
        _ => emit_ir_operand(operand, strings, label_name, stack),
    }
}

fn emit_ir_float_copy_instruction(
    asm: &mut String,
    src: &ir::Operand,
    dst: &ir::Operand,
    width: MemoryWidth,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let src = emit_ir_float_operand(src, width, strings, label_name, stack)?;
    let dst = emit_ir_operand(dst, strings, label_name, stack)?;
    asm::float_move(
        asm,
        x86_float_move_opcode(width)?.to_owned(),
        asm::Operand::Address(dst),
        asm::Operand::Address(src),
    );
    Ok(())
}

fn ir_binary_may_be_float(lhs: &ir::Operand, rhs: &ir::Operand) -> bool {
    [lhs, rhs].into_iter().any(|operand| {
        matches!(operand, ir::Operand::FloatLiteral(_))
            || matches!(operand, ir::Operand::TargetRegister(name) if is_xmm_register(name))
    })
}

fn emit_ir_integer_binary_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    op: MathOp,
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if lhs == dst {
        return emit_ir_integer_math_instruction(asm, op, rhs, dst, strings, label_name, stack);
    }

    if rhs == dst {
        if is_commutative_math_op(op) {
            return emit_ir_integer_math_instruction(asm, op, lhs, dst, strings, label_name, stack);
        }
        if op == MathOp::Subtract {
            let dst_text = emit_ir_operand(dst, strings, label_name, stack)?;
            asm::instruction(asm, format_args!("neg {dst_text}"));
            return emit_ir_binary_instruction(asm, "add", lhs, dst, strings, label_name, stack);
        }
        return Err(format!(
            "Binary assignment destination cannot also be the right operand for {}",
            math_op_symbol(op)
        ));
    }

    emit_ir_binary_instruction(asm, "mov", lhs, dst, strings, label_name, stack)?;
    emit_ir_integer_math_instruction(asm, op, rhs, dst, strings, label_name, stack)
}

fn emit_ir_integer_math_instruction(
    asm: &mut String,
    op: MathOp,
    src: &ir::Operand,
    dst: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    emit_ir_binary_instruction(
        asm,
        integer_math_opcode(op),
        src,
        dst,
        strings,
        label_name,
        stack,
    )
}

fn emit_ir_binary_instruction(
    asm: &mut String,
    opcode: &str,
    src: &ir::Operand,
    dst: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    asm::binary(
        asm,
        opcode,
        ir_machine_operand(dst, strings, label_name, stack)?,
        ir_machine_operand(src, strings, label_name, stack)?,
    );
    Ok(())
}

fn emit_ir_copy_instruction(
    asm: &mut String,
    src: &ir::Operand,
    dst: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if src == dst {
        return Ok(());
    }

    let dst_machine = ir_machine_operand(dst, strings, label_name, stack)?;
    let narrowed_src = match (
        src,
        dst,
        ir_operand_width(src, strings, label_name, stack),
        ir_operand_width(dst, strings, label_name, stack),
    ) {
        (ir::Operand::TargetRegister(register), _, Some(src_width), Some(dst_width))
            if src_width.bits() > dst_width.bits() =>
        {
            ir::Operand::TargetRegister(register_alias(register, dst_width)?.to_owned())
        }
        _ => src.clone(),
    };
    let src_machine = ir_machine_operand(&narrowed_src, strings, label_name, stack)?;
    match (&dst_machine, &src_machine) {
        (asm::Operand::Address(_) | asm::Operand::Memory(_), _) => {
            asm::store(asm, dst_machine, src_machine)
        }
        (_, asm::Operand::Address(_) | asm::Operand::Memory(_)) => {
            asm::load(asm, dst_machine, src_machine)
        }
        _ => asm::mov(asm, dst_machine, src_machine),
    }
    Ok(())
}

fn ir_machine_operand(
    operand: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<asm::Operand, String> {
    match operand {
        ir::Operand::Immediate(value) => Ok(asm::Operand::Immediate(*value)),
        ir::Operand::TargetRegister(name) => Ok(asm::Operand::Register(name.clone())),
        ir::Operand::Memory {
            address,
            width: Some(width),
        } => Ok(asm::Operand::Memory(ir_machine_memory_address(
            address,
            Some(*width),
            stack,
        ))),
        _ => Ok(asm::Operand::Address(emit_ir_operand(
            operand, strings, label_name, stack,
        )?)),
    }
}

fn ir_machine_memory_address(
    address: &ir::Address,
    width: Option<MemoryWidth>,
    stack: &StackFrame,
) -> asm::MemoryAddress {
    asm::MemoryAddress {
        width: width.map(|width| width.ptr().to_owned()),
        terms: std::iter::once((
            asm::AddressOperator::Add,
            ir_machine_address_term(&address.first, stack),
        ))
        .chain(address.rest.iter().map(|(operator, term)| {
            (
                match operator {
                    ir::AddressOperator::Add => asm::AddressOperator::Add,
                    ir::AddressOperator::Subtract => asm::AddressOperator::Subtract,
                },
                ir_machine_address_term(term, stack),
            )
        }))
        .collect(),
    }
}

fn ir_machine_address_term(term: &ir::AddressTerm, stack: &StackFrame) -> asm::AddressTerm {
    match term {
        ir::AddressTerm::Immediate(value) => asm::AddressTerm::Immediate(*value),
        ir::AddressTerm::Name(name) => stack_buffer_slot(stack, name)
            .map(|(offset, _)| asm::AddressTerm::Register(format!("rbp - {offset}")))
            .unwrap_or_else(|| asm::AddressTerm::Symbol(name.clone())),
        ir::AddressTerm::TargetRegister(register) => asm::AddressTerm::Register(register.clone()),
        ir::AddressTerm::ScaledTargetRegister { register, scale } => {
            asm::AddressTerm::ScaledRegister {
                register: register.clone(),
                scale: *scale,
            }
        }
    }
}

fn emit_ir_boolean_condition_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    condition: &ir::Condition,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let set_opcode = match condition {
        ir::Condition::Compare { lhs, op, rhs } => {
            let (lhs, rhs, op) = normalize_ir_compare(lhs, rhs, *op, strings, label_name)?;
            validate_resolved_integer_compare_op(op)?;
            validate_ir_compare_operands(lhs, rhs, strings, label_name, stack)?;
            let use_test = matches!(op, CompareOp::Equal | CompareOp::NotEqual)
                && matches!(lhs, ir::Operand::TargetRegister(register) if !is_xmm_register(register))
                && matches!(rhs, ir::Operand::Immediate(0));
            let lhs = emit_ir_operand(lhs, strings, label_name, stack)?;
            let rhs = emit_ir_operand(rhs, strings, label_name, stack)?;
            if use_test {
                asm::instruction(asm, format_args!("test {lhs}, {lhs}"));
            } else {
                asm::instruction(asm, format_args!("cmp {lhs}, {rhs}"));
            }
            compare_set_opcode(op)
        }
        ir::Condition::BitwiseAndZero { lhs, rhs, op } => {
            validate_ir_test_condition_operands(lhs, rhs, *op, strings, label_name, stack)?;
            let lhs = emit_ir_operand(lhs, strings, label_name, stack)?;
            let rhs = emit_ir_operand(rhs, strings, label_name, stack)?;
            asm::instruction(asm, format_args!("test {lhs}, {rhs}"));
            match op {
                CompareOp::Equal => "sete",
                CompareOp::NotEqual => "setne",
                _ => unreachable!(),
            }
        }
    };

    emit_ir_setcc_result(asm, set_opcode, dst, strings, label_name, stack)
}

fn validate_ir_condition_operand(
    operand: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let _ = ir_machine_operand(operand, strings, label_name, stack)?;
    Ok(())
}

fn validate_ir_compare_operands(
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if ir_operand_is_float(lhs, strings, label_name, stack)
        || ir_operand_is_float(rhs, strings, label_name, stack)
    {
        return Err(String::from(
            "Floating-point operands cannot be compared yet",
        ));
    }
    if matches!(lhs, ir::Operand::Pointer(_) | ir::Operand::AddressOf(_))
        || matches!(rhs, ir::Operand::Pointer(_) | ir::Operand::AddressOf(_))
    {
        return Err(String::from("Comparison cannot use an address-of operand"));
    }
    if ir_operand_is_memory(lhs, stack) && ir_operand_is_memory(rhs, stack) {
        return Err(String::from(
            "Comparison cannot use memory for both operands",
        ));
    }
    if let (Some(lhs_width), Some(rhs_width)) = (
        ir_operand_width(lhs, strings, label_name, stack),
        ir_operand_width(rhs, strings, label_name, stack),
    ) && lhs_width != rhs_width
    {
        return Err(format!(
            "Cannot compare {}-bit operand with {}-bit operand",
            lhs_width.bits(),
            rhs_width.bits()
        ));
    }
    if let (Some(value), Some(width)) = (
        ir_operand_immediate_value(rhs, strings, label_name),
        ir_operand_width(lhs, strings, label_name, stack),
    ) {
        validate_immediate_range(value, ImmediateDestination::Register(width))?;
    }
    if ir_operand_immediate_value(rhs, strings, label_name).is_some()
        && matches!(lhs, ir::Operand::Memory { width: None, .. })
        && ir_operand_memory_width(lhs, strings, stack).is_none()
    {
        return Err(String::from(
            "Cannot compare an immediate value with memory without an explicit width",
        ));
    }
    Ok(())
}

fn validate_ir_test_condition_operands(
    lhs: &ir::Operand,
    rhs: &ir::Operand,
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
    validate_ir_copy_assignment(rhs, lhs, strings, label_name, stack)
}

fn validate_ir_boolean_destination(
    dst: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(
        dst,
        ir::Operand::Immediate(_)
            | ir::Operand::Pointer(_)
            | ir::Operand::AddressOf(_)
            | ir::Operand::StringProperty { .. }
            | ir::Operand::FloatLiteral(_)
    ) || matches!(dst, ir::Operand::Name(name) if stack_scalar_slot(stack, name).is_none())
    {
        return Err(String::from(
            "Boolean assignment destination must be a register or integer memory operand",
        ));
    }
    if matches!(dst, ir::Operand::TargetRegister(name) if is_xmm_register(name))
        || ir_operand_is_float(dst, strings, label_name, stack)
    {
        return Err(String::from(
            "Boolean assignment destination must be an integer register or memory operand",
        ));
    }
    Ok(())
}

fn emit_ir_setcc_result(
    asm: &mut String,
    set_opcode: &str,
    dst: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_ir_boolean_destination(dst, strings, label_name, stack)?;
    let width = ir_operand_width(dst, strings, label_name, stack).ok_or_else(|| {
        String::from("Boolean assignment destination must have a known integer width")
    })?;
    if let ir::Operand::TargetRegister(register) = dst
        && width == Width::Bits8
    {
        asm::instruction(asm, format_args!("{set_opcode} {register}"));
        return Ok(());
    }

    let temp = if !ir_operand_address_uses_register_family(
        dst,
        &ir::Operand::TargetRegister(String::from("r10")),
    ) {
        "r10"
    } else if !ir_operand_address_uses_register_family(
        dst,
        &ir::Operand::TargetRegister(String::from("r11")),
    ) {
        "r11"
    } else {
        return Err(String::from(
            "Boolean assignment destination address cannot use both r10 and r11",
        ));
    };
    asm::instruction(asm, format_args!("{set_opcode} {temp}b"));
    let emitted_dst = emit_ir_operand(dst, strings, label_name, stack)?;
    if ir_operand_is_memory(dst, stack) {
        let suffix = match width {
            Width::Bits8 => "b",
            Width::Bits16 => "w",
            Width::Bits32 => "d",
            Width::Bits64 => "",
        };
        asm::instruction(asm, format_args!("mov {emitted_dst}, {temp}{suffix}"));
    } else {
        asm::instruction(asm, format_args!("movzx {emitted_dst}, {temp}b"));
    }
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

fn emit_ir_intrinsic_call_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    op: IntrinsicOp,
    width: MemoryWidth,
    args: &[ir::Operand],
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match op {
        IntrinsicOp::Ceil | IntrinsicOp::Floor | IntrinsicOp::Round | IntrinsicOp::Trunc => {
            if !matches!(width, MemoryWidth::F32 | MemoryWidth::F64) {
                return Err(format!(
                    "{} only supports f32 or f64; integer rounding is not implemented",
                    intrinsic_op_name(op)
                ));
            }
            let src = args.first().ok_or_else(|| {
                format!("{} intrinsic requires an operand", intrinsic_op_name(op))
            })?;
            emit_ir_float_rounding_intrinsic(asm, dst, op, width, src, strings, label_name, stack)
        }
        IntrinsicOp::Sqrt => {
            let src = args
                .first()
                .ok_or_else(|| String::from("sqrt requires an operand"))?;
            match width {
                MemoryWidth::F32 | MemoryWidth::F64 => {
                    emit_ir_float_sqrt_intrinsic(asm, dst, width, src, strings, label_name, stack)
                }
                MemoryWidth::I8
                | MemoryWidth::I16
                | MemoryWidth::I32
                | MemoryWidth::I64
                | MemoryWidth::U8
                | MemoryWidth::U16
                | MemoryWidth::U32
                | MemoryWidth::U64 => {
                    emit_ir_integer_sqrt_intrinsic(asm, dst, width, src, strings, label_name, stack)
                }
                _ => Err(String::from(
                    "sqrt integer operands must use a signed or unsigned integer width",
                )),
            }
        }
        IntrinsicOp::Min | IntrinsicOp::Max => {
            let lhs = args.first().ok_or_else(|| {
                format!("{} intrinsic requires two operands", intrinsic_op_name(op))
            })?;
            let rhs = args.get(1).ok_or_else(|| {
                format!("{} intrinsic requires two operands", intrinsic_op_name(op))
            })?;
            match width {
                MemoryWidth::F32 | MemoryWidth::F64 => emit_ir_float_min_max_intrinsic(
                    asm, dst, op, width, lhs, rhs, strings, label_name, stack,
                ),
                MemoryWidth::Ptr => Err(String::from(
                    "min and max do not support ptr width; use an integer width",
                )),
                _ => emit_ir_integer_min_max_intrinsic(
                    asm, dst, op, width, lhs, rhs, strings, label_name, stack,
                ),
            }
        }
    }
}

fn emit_ir_integer_sqrt_intrinsic(
    asm: &mut String,
    dst: &ir::Operand,
    width: MemoryWidth,
    src: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let expected = memory_width_bits(width);
    let (dst_register, memory_destination) = match dst {
        ir::Operand::TargetRegister(dst_register) if !is_xmm_register(dst_register) => {
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
            if is_high_byte_register(dst_register) {
                return Err(String::from(
                    "Integer sqrt intrinsic destination cannot use a high-byte register",
                ));
            }
            (Some(dst_register.as_str()), false)
        }
        ir::Operand::TargetRegister(_) => {
            return Err(String::from(
                "Integer sqrt intrinsic destination must be an integer register or memory operand",
            ));
        }
        dst if ir_operand_is_memory(dst, stack)
            && !matches!(dst, ir::Operand::StringProperty { .. }) =>
        {
            if ir_operand_is_float(dst, strings, label_name, stack) {
                return Err(String::from(
                    "Integer sqrt intrinsic destination must be an integer memory operand",
                ));
            }
            let Some(dst_width) = ir_operand_width(dst, strings, label_name, stack) else {
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

    if ir_operand_is_float(src, strings, label_name, stack)
        || matches!(
            src,
            ir::Operand::Pointer(_)
                | ir::Operand::AddressOf(_)
                | ir::Operand::StringProperty { .. }
                | ir::Operand::Converted { .. }
                | ir::Operand::Cast { .. }
                | ir::Operand::FloatLiteral(_)
        )
    {
        return Err(String::from(
            "Integer sqrt intrinsic operand must be an integer operand",
        ));
    }
    if let Some(src_width) = ir_operand_width(src, strings, label_name, stack)
        && src_width != expected
    {
        return Err(format!(
            "Integer sqrt intrinsic operand must be {}-bit, found {}-bit operand",
            expected.bits(),
            src_width.bits()
        ));
    }

    if let Some(value) = ir_operand_immediate_value(src, strings, label_name) {
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
                None => !ir_operand_address_uses_register_family(
                    dst,
                    &ir::Operand::TargetRegister((*register).to_owned()),
                ),
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

    emit_ir_integer_sqrt_source_load(asm, base, src, expected, strings, label_name, stack)?;

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
        asm::instruction(asm, format_args!("bt {base}, {}", expected.bits() - 1));
        asm::branch(asm, "jc", asm::Operand::Address(negative_label.clone()));
    }
    asm::instruction(
        asm,
        format_args!("xor {dst_accumulator}, {dst_accumulator}"),
    );
    asm::instruction(asm, format_args!("mov {bit}, {bit_value}"));
    asm::label(asm, &find_bit_label);
    asm::instruction(asm, format_args!("cmp {base}, {bit}"));
    asm::branch(asm, "jae", asm::Operand::Address(loop_label.clone()));
    asm::instruction(asm, format_args!("shr {bit}, 2"));
    asm::instruction(asm, format_args!("test {bit}, {bit}"));
    asm::branch(asm, "jne", asm::Operand::Address(find_bit_label.clone()));
    asm::jump(asm, asm::Operand::Address(done_label.clone()));
    asm::label(asm, &loop_label);
    asm::instruction(asm, format_args!("test {bit}, {bit}"));
    asm::branch(asm, "je", asm::Operand::Address(done_label.clone()));
    asm::lea(
        asm,
        asm::Operand::Register(sum.to_owned()),
        format!("[{dst_accumulator} + {bit}]"),
    );
    asm::instruction(asm, format_args!("cmp {base}, {sum}"));
    asm::branch(asm, "jb", asm::Operand::Address(no_subtract_label.clone()));
    asm::instruction(asm, format_args!("sub {base}, {sum}"));
    asm::instruction(asm, format_args!("shr {dst_accumulator}, 1"));
    asm::instruction(asm, format_args!("add {dst_accumulator}, {bit}"));
    asm::instruction(asm, format_args!("shr {bit}, 2"));
    asm::jump(asm, asm::Operand::Address(loop_label.clone()));
    asm::label(asm, &no_subtract_label);
    asm::instruction(asm, format_args!("shr {dst_accumulator}, 1"));
    asm::instruction(asm, format_args!("shr {bit}, 2"));
    asm::jump(asm, asm::Operand::Address(loop_label.clone()));
    asm::label(asm, &done_label);

    if narrow_destination {
        let result = register_alias(&dst_accumulator, expected)?;
        asm::instruction(asm, format_args!("mov {}, {result}", dst_register.unwrap()));
    } else if memory_destination {
        let result = ir::Operand::TargetRegister(register_alias(&dst_accumulator, expected)?);
        emit_ir_copy_instruction(asm, &result, dst, strings, label_name, stack)?;
    }
    if is_signed_integer_width(width) {
        asm::jump(asm, asm::Operand::Address(after_negative_label.clone()));
        asm::label(asm, &negative_label);
        asm::instruction(asm, "ud2");
        asm::label(asm, &after_negative_label);
    }
    Ok(())
}

fn emit_ir_integer_sqrt_source_load(
    asm: &mut String,
    base: &str,
    src: &ir::Operand,
    width: Width,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if ir_operand_uses_high_byte(src) {
        return Err(String::from(
            "Integer sqrt intrinsic cannot combine high-byte registers with extended scratch registers",
        ));
    }

    let emitted = emit_ir_operand(src, strings, label_name, stack)?;
    match width {
        Width::Bits64 => asm::instruction(asm, format_args!("mov {base}, {emitted}")),
        Width::Bits32 => {
            let source = match src {
                ir::Operand::TargetRegister(register) => register_alias(register, Width::Bits32)?,
                _ => emitted,
            };
            asm::instruction(asm, format_args!("mov {base}d, {source}"));
        }
        Width::Bits16 | Width::Bits8 => {
            if ir_operand_immediate_value(src, strings, label_name).is_some() {
                asm::instruction(asm, format_args!("mov {base}, {emitted}"));
            } else {
                asm::instruction(asm, format_args!("movzx {base}, {emitted}"));
            }
        }
    }
    Ok(())
}

fn emit_ir_float_rounding_intrinsic(
    asm: &mut String,
    dst: &ir::Operand,
    op: IntrinsicOp,
    width: MemoryWidth,
    src: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let memory_destination = validate_ir_float_intrinsic_destination(
        intrinsic_op_name(op),
        dst,
        width,
        strings,
        label_name,
        stack,
    )?;
    validate_ir_float_math_operand(
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
        let ir::Operand::TargetRegister(register) = dst else {
            unreachable!()
        };
        register.as_str()
    };
    let src = emit_ir_float_operand(src, width, strings, label_name, stack)?;
    asm::instruction(
        asm,
        format_args!(
            "{} {dst_register}, {src}, {}",
            float_rounding_opcode(width),
            float_rounding_mode(op)
        ),
    );

    if memory_destination {
        emit_ir_float_copy_instruction(
            asm,
            &ir::Operand::TargetRegister(String::from("xmm15")),
            dst,
            width,
            strings,
            label_name,
            stack,
        )?;
    }
    Ok(())
}

fn emit_ir_float_sqrt_intrinsic(
    asm: &mut String,
    dst: &ir::Operand,
    width: MemoryWidth,
    src: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let memory_destination =
        validate_ir_float_intrinsic_destination("sqrt", dst, width, strings, label_name, stack)?;
    validate_ir_float_math_operand("sqrt operand", src, width, strings, label_name, stack)?;

    let dst_register = if memory_destination {
        "xmm15"
    } else {
        let ir::Operand::TargetRegister(register) = dst else {
            unreachable!()
        };
        register.as_str()
    };
    let src = emit_ir_float_operand(src, width, strings, label_name, stack)?;
    asm::instruction(
        asm,
        format_args!("{} {dst_register}, {src}", float_sqrt_opcode(width)),
    );

    if memory_destination {
        emit_ir_float_copy_instruction(
            asm,
            &ir::Operand::TargetRegister(String::from("xmm15")),
            dst,
            width,
            strings,
            label_name,
            stack,
        )?;
    }
    Ok(())
}

fn emit_ir_float_min_max_intrinsic(
    asm: &mut String,
    dst: &ir::Operand,
    op: IntrinsicOp,
    width: MemoryWidth,
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let memory_destination = validate_ir_float_intrinsic_destination(
        intrinsic_op_name(op),
        dst,
        width,
        strings,
        label_name,
        stack,
    )?;
    validate_ir_float_math_operand(
        "Floating-point intrinsic left operand",
        lhs,
        width,
        strings,
        label_name,
        stack,
    )?;
    validate_ir_float_math_operand(
        "Floating-point intrinsic right operand",
        rhs,
        width,
        strings,
        label_name,
        stack,
    )?;

    let target = if memory_destination {
        ir::Operand::TargetRegister(String::from("xmm15"))
    } else {
        dst.clone()
    };
    if lhs != &target {
        emit_ir_float_copy_instruction(asm, lhs, &target, width, strings, label_name, stack)?;
    }
    let ir::Operand::TargetRegister(register) = &target else {
        unreachable!()
    };
    let rhs = emit_ir_float_operand(rhs, width, strings, label_name, stack)?;
    asm::instruction(
        asm,
        format_args!("{} {register}, {rhs}", float_min_max_opcode(op, width)),
    );
    if memory_destination {
        emit_ir_float_copy_instruction(asm, &target, dst, width, strings, label_name, stack)?;
    }
    Ok(())
}

fn emit_ir_integer_min_max_intrinsic(
    asm: &mut String,
    dst: &ir::Operand,
    op: IntrinsicOp,
    width: MemoryWidth,
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    validate_ir_integer_min_max_intrinsic(dst, width, lhs, rhs, strings, label_name, stack)?;

    let intrinsic_width = memory_width_bits(width);
    let memory_destination = !matches!(dst, ir::Operand::TargetRegister(_));
    let result_register = if memory_destination {
        ir_integer_memory_result_register(dst, rhs)?
    } else {
        let ir::Operand::TargetRegister(register) = dst else {
            unreachable!()
        };
        register.clone()
    };
    let result_register = register_alias(&result_register, intrinsic_width)?;
    let result = ir::Operand::TargetRegister(result_register.clone());
    emit_ir_copy_instruction(asm, lhs, &result, strings, label_name, stack)?;

    let rhs = match rhs {
        ir::Operand::TargetRegister(register)
            if register_width(register).is_some_and(|rhs_width| rhs_width != intrinsic_width) =>
        {
            ir::Operand::TargetRegister(register_alias(register, intrinsic_width)?)
        }
        _ => rhs.clone(),
    };
    let dst_operand = emit_ir_operand(&result, strings, label_name, stack)?;
    let rhs_operand = emit_ir_operand(&rhs, strings, label_name, stack)?;
    let keep_label = format!(
        ".L.__subsea.{label_name}.{}_{}_keep",
        intrinsic_op_name(op),
        asm.len()
    );
    asm::instruction(asm, format_args!("cmp {dst_operand}, {rhs_operand}"));
    asm::branch(
        asm,
        integer_min_max_keep_jump(op, width),
        asm::Operand::Address(keep_label.clone()),
    );
    asm::instruction(asm, format_args!("mov {dst_operand}, {rhs_operand}"));
    asm::label(asm, &keep_label);

    if memory_destination {
        emit_ir_copy_instruction(asm, &result, dst, strings, label_name, stack)?;
    }
    Ok(())
}

fn validate_ir_integer_min_max_intrinsic(
    dst: &ir::Operand,
    width: MemoryWidth,
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    let expected = memory_width_bits(width);
    let dst_width = match dst {
        ir::Operand::TargetRegister(register) if !is_xmm_register(register) => {
            register_width(register).ok_or_else(|| {
                String::from("Integer min/max intrinsic destination must be a register")
            })?
        }
        ir::Operand::TargetRegister(_) => {
            return Err(String::from(
                "Integer min/max intrinsic destination must be an integer register",
            ));
        }
        dst if ir_operand_is_memory(dst, stack)
            && !matches!(dst, ir::Operand::StringProperty { .. }) =>
        {
            if ir_operand_is_float(dst, strings, label_name, stack) {
                return Err(String::from(
                    "Integer min/max intrinsic destination must be integer memory",
                ));
            }
            ir_operand_width(dst, strings, label_name, stack).ok_or_else(|| {
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
    validate_ir_integer_min_max_operand(
        "Integer min/max intrinsic left operand",
        lhs,
        expected,
        strings,
        label_name,
        stack,
    )?;
    validate_ir_integer_min_max_operand(
        "Integer min/max intrinsic right operand",
        rhs,
        expected,
        strings,
        label_name,
        stack,
    )?;
    if ir_operand_uses_high_byte(lhs) && ir_operand_uses_extended(dst)
        || ir_operand_uses_high_byte(rhs) && ir_operand_uses_extended(dst)
    {
        return Err(String::from(
            "Integer min/max intrinsic cannot combine high-byte registers ah, bh, ch, or dh with extended registers",
        ));
    }
    Ok(())
}

fn validate_ir_integer_min_max_operand(
    name: &str,
    operand: &ir::Operand,
    expected: Width,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if ir_operand_is_float(operand, strings, label_name, stack)
        || matches!(
            operand,
            ir::Operand::Pointer(_)
                | ir::Operand::AddressOf(_)
                | ir::Operand::StringProperty { .. }
                | ir::Operand::Converted { .. }
                | ir::Operand::Cast { .. }
                | ir::Operand::FloatLiteral(_)
        )
    {
        return Err(format!("{name} must be an integer operand"));
    }
    if let Some(width) = ir_operand_width(operand, strings, label_name, stack)
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

fn validate_ir_wide_math_operand(
    name: &str,
    operand: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(operand, ir::Operand::Pointer(_)) {
        return Err(format!("{name} cannot be an address-of operand"));
    }
    if let Some(width) = ir_operand_width(operand, strings, label_name, stack)
        && width != Width::Bits64
    {
        return Err(format!(
            "{name} must be 64-bit, found {}-bit operand",
            width.bits()
        ));
    }
    Ok(())
}

fn validate_ir_push_operand(
    src: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(src, ir::Operand::Pointer(_) | ir::Operand::AddressOf(_)) {
        return Err(String::from("push source cannot be an address-of operand"));
    }
    if ir_operand_immediate_value(src, strings, label_name).is_some() {
        return Ok(());
    }
    validate_ir_stack_width("push source", src, strings, label_name, stack)
}

fn validate_ir_pop_operand(
    dst: &ir::Operand,
    strings: &StringTable,
    stack: &StackFrame,
) -> Result<(), String> {
    if matches!(
        dst,
        ir::Operand::Immediate(_)
            | ir::Operand::Pointer(_)
            | ir::Operand::AddressOf(_)
            | ir::Operand::StringProperty { .. }
    ) {
        return Err(String::from(
            "pop destination must be a 64-bit register or explicitly 64-bit memory operand",
        ));
    }
    validate_ir_stack_width("pop destination", dst, strings, "", stack)
}

fn validate_ir_stack_width(
    name: &str,
    operand: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<(), String> {
    match operand {
        ir::Operand::TargetRegister(register) if is_xmm_register(register) => Err(format!(
            "{name} must be a 64-bit integer register, found XMM register {register}"
        )),
        ir::Operand::TargetRegister(register) => match register_width(register) {
            Some(Width::Bits64) => Ok(()),
            Some(width) => Err(format!(
                "{name} must be 64-bit, found {}-bit register",
                width.bits()
            )),
            None => Ok(()),
        },
        ir::Operand::Memory { .. } => match ir_operand_memory_width(operand, strings, stack) {
            Some(width) if memory_width_bits(width) == Width::Bits64 => Ok(()),
            Some(width) => Err(format!(
                "{name} must be 64-bit, found {}-bit memory operand",
                memory_width_bits(width).bits()
            )),
            None => Err(format!(
                "{name} memory operand requires an explicit 64-bit width"
            )),
        },
        ir::Operand::Name(binding) if stack_scalar_slot(stack, binding).is_some() => {
            match ir_operand_width(operand, strings, label_name, stack) {
                Some(Width::Bits64) | None => Ok(()),
                Some(width) => Err(format!(
                    "{name} must be 64-bit, found {}-bit stack variable",
                    width.bits()
                )),
            }
        }
        _ => Ok(()),
    }
}

fn ir_integer_memory_result_register(
    dst: &ir::Operand,
    rhs: &ir::Operand,
) -> Result<String, String> {
    ["r10", "r11", "r8", "r9"]
        .into_iter()
        .find(|register| {
            !ir_operand_address_uses_register_family(
                dst,
                &ir::Operand::TargetRegister((*register).to_owned()),
            ) && !ir_operand_uses_register_family(rhs, register)
        })
        .map(String::from)
        .ok_or_else(|| {
            String::from("Integer min/max intrinsic has no available result scratch register")
        })
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

fn register_alias(name: &str, width: Width) -> Result<String, String> {
    let family =
        register_family(name).ok_or_else(|| format!("Expected integer register, found {name}"))?;

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

fn same_register_family(left: &str, right: &str) -> bool {
    register_family(left).is_some_and(|family| register_family(right) == Some(family))
}

pub(crate) fn emit_ir_operand(
    operand: &ir::Operand,
    strings: &StringTable,
    label_name: &str,
    stack: &StackFrame,
) -> Result<String, String> {
    match operand {
        ir::Operand::Immediate(value) => Ok(value.to_string()),
        ir::Operand::Name(name) => match stack_scalar_slot(stack, name) {
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
        ir::Operand::Memory { address, .. } => {
            let emitted_address = emit_ir_address(address, stack);
            Ok(match ir_operand_memory_width(operand, strings, stack) {
                Some(width) => format!("{} ptr [{}]", width.ptr(), emitted_address),
                None => format!("[{emitted_address}]"),
            })
        }
        ir::Operand::StringProperty { name, property } => {
            let offset = match (stack_string_slot(stack, name), property) {
                (Some((_, len_offset)), ir::StringProperty::Len) => Some(len_offset),
                (Some((ptr_offset, _)), ir::StringProperty::Ptr) => Some(ptr_offset),
                _ => None,
            };
            if let Some(offset) = offset {
                return Ok(format!("qword ptr [rbp - {offset}]"));
            }

            let binding = strings
                .bindings
                .get(&(label_name.to_string(), name.clone()))
                .ok_or_else(|| {
                    format!("Unknown string binding {name:?} in label {label_name:?}")
                })?;

            Ok(match property {
                ir::StringProperty::Len => binding.value.len().to_string(),
                ir::StringProperty::Ptr => format!("offset {}", binding.asm_label),
            })
        }
        ir::Operand::TargetRegister(name) => Ok(name.clone()),
        ir::Operand::FloatLiteral(value) => Err(format!(
            "Float literal {value} requires an explicit floating-point operator width"
        )),
        ir::Operand::Pointer(name) => Err(format!(
            "Pointer operand &{name} is only supported as the right side of assignment"
        )),
        ir::Operand::AddressOf(_) => Err(String::from(
            "Address-of operands are only supported as assignment sources",
        )),
        ir::Operand::Converted { .. } | ir::Operand::Cast { .. } => Err(String::from(
            "Conversion operands are only supported as assignment sources",
        )),
    }
}

fn emit_ir_address(address: &ir::Address, stack: &StackFrame) -> String {
    let mut value = emit_ir_address_term(&address.first, stack);

    for (operator, term) in &address.rest {
        value.push_str(match operator {
            ir::AddressOperator::Add => " + ",
            ir::AddressOperator::Subtract => " - ",
        });
        value.push_str(&emit_ir_address_term(term, stack));
    }

    value
}

fn emit_ir_address_term(term: &ir::AddressTerm, stack: &StackFrame) -> String {
    match term {
        ir::AddressTerm::Immediate(value) => value.to_string(),
        ir::AddressTerm::Name(name) => stack_buffer_slot(stack, name)
            .map(|(offset, _)| format!("rbp - {offset}"))
            .unwrap_or_else(|| name.clone()),
        ir::AddressTerm::TargetRegister(name) => name.clone(),
        ir::AddressTerm::ScaledTargetRegister { register, scale } => {
            format!("{register} * {scale}")
        }
    }
}

pub(crate) fn integer_math_opcode(op: MathOp) -> &'static str {
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

pub(crate) fn bitwise_unary_opcode(op: BitwiseUnaryOp) -> &'static str {
    match op {
        BitwiseUnaryOp::Not => "not",
    }
}

pub(crate) fn compare_jump_opcode(op: CompareOp) -> &'static str {
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

pub(crate) fn compare_set_opcode(op: CompareOp) -> &'static str {
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

pub(crate) fn float_sqrt_opcode(width: MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::F32 => "sqrtss",
        MemoryWidth::F64 => "sqrtsd",
        _ => unreachable!(),
    }
}

pub(crate) fn float_rounding_opcode(width: MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::F32 => "roundss",
        MemoryWidth::F64 => "roundsd",
        _ => unreachable!(),
    }
}

pub(crate) fn float_rounding_mode(op: IntrinsicOp) -> u8 {
    match op {
        IntrinsicOp::Round => 0,
        IntrinsicOp::Floor => 1,
        IntrinsicOp::Ceil => 2,
        IntrinsicOp::Trunc => 3,
        _ => unreachable!(),
    }
}

pub(crate) fn float_min_max_opcode(op: IntrinsicOp, width: MemoryWidth) -> &'static str {
    match (op, width) {
        (IntrinsicOp::Min, MemoryWidth::F32) => "minss",
        (IntrinsicOp::Min, MemoryWidth::F64) => "minsd",
        (IntrinsicOp::Max, MemoryWidth::F32) => "maxss",
        (IntrinsicOp::Max, MemoryWidth::F64) => "maxsd",
        _ => unreachable!(),
    }
}

pub(crate) fn float_math_opcode(op: FloatMathOp, width: MemoryWidth) -> &'static str {
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

pub(crate) fn float_compare_opcode(width: MemoryWidth) -> &'static str {
    match width {
        MemoryWidth::F32 => "ucomiss",
        MemoryWidth::F64 => "ucomisd",
        _ => unreachable!(),
    }
}

pub(crate) fn float_compare_jump_opcode(op: CompareOp) -> &'static str {
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

pub(crate) fn division_opcode(signed: bool) -> &'static str {
    if signed { "idiv" } else { "div" }
}

pub(crate) fn wide_math_opcode(division: bool, signed: bool) -> &'static str {
    match (division, signed) {
        (false, true) => "imul",
        (false, false) => "mul",
        (true, true) => "idiv",
        (true, false) => "div",
    }
}

pub(crate) fn pair_math_opcodes(op: crate::ast::PairBinaryOp) -> (&'static str, &'static str) {
    match op {
        crate::ast::PairBinaryOp::Add => ("add", "adc"),
        crate::ast::PairBinaryOp::Subtract => ("sub", "sbb"),
    }
}

pub(crate) fn x86_float_move_opcode(width: MemoryWidth) -> Result<&'static str, String> {
    match width {
        MemoryWidth::F32 => Ok("movss"),
        MemoryWidth::F64 => Ok("movsd"),
        _ => Err(String::from(
            "XMM moves require an explicitly f32 or f64 memory operand",
        )),
    }
}

pub(crate) fn emit_frame_prologue(asm: &mut String, stack: &StackFrame, spec: TargetSpec) {
    asm::push(asm, asm::Operand::Register(spec.frame_pointer.to_owned()));
    asm::mov(
        asm,
        asm::Operand::Register(spec.frame_pointer.to_owned()),
        asm::Operand::Register(spec.stack_pointer.to_owned()),
    );
    if stack.size > 0 {
        asm::stack_adjust(asm, "sub", spec.stack_pointer.to_owned(), stack.size);
    }
}

pub(crate) fn emit_frame_epilogue(asm: &mut String, spec: TargetSpec) {
    asm::mov(
        asm,
        asm::Operand::Register(spec.stack_pointer.to_owned()),
        asm::Operand::Register(spec.frame_pointer.to_owned()),
    );
    asm::pop(asm, asm::Operand::Register(spec.frame_pointer.to_owned()));
}

pub(crate) fn emit_linux_syscall(asm: &mut String, number: u64) {
    asm::syscall(asm, number);
}

pub(crate) fn emit_linux_write_label(asm: &mut String, label: &str, len: usize) {
    asm::mov(
        asm,
        asm::Operand::Register(String::from("rax")),
        asm::Operand::Immediate(linux::SYS_WRITE as i128),
    );
    asm::instruction(
        asm,
        format_args!(
            "mov rdi, {}\n  lea rsi, [rip + {label}]\n  mov rdx, {len}",
            linux::STDOUT
        ),
    );
    asm::syscall_trap(asm);
}

pub(crate) fn emit_linux_write_registers(asm: &mut String) {
    asm::mov(
        asm,
        asm::Operand::Register(String::from("rax")),
        asm::Operand::Immediate(linux::SYS_WRITE as i128),
    );
    asm::mov(
        asm,
        asm::Operand::Register(String::from("rdi")),
        asm::Operand::Immediate(linux::STDOUT as i128),
    );
    asm::syscall_trap(asm);
}

pub(crate) fn emit_linux_read(asm: &mut String) {
    emit_linux_syscall(asm, linux::SYS_READ);
}

pub(crate) fn emit_linux_mmap(asm: &mut String) {
    emit_linux_syscall(asm, linux::SYS_MMAP);
}

pub(crate) fn emit_linux_munmap(asm: &mut String) {
    emit_linux_syscall(asm, linux::SYS_MUNMAP);
}

#[cfg(test)]
#[path = "codegen_tests.rs"]
mod tests;
