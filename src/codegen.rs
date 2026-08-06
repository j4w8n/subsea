use crate::ast::{
    Address, AddressOperator, AddressTerm, AssignmentValue, BindingValue, Instruction, MathOp,
    MemoryDeclaration, MemoryWidth, Operand, PrintPart, Program,
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
        asm.push_str(&format!("{}:\n", label.name));

        for instruction in &label.instructions {
            match instruction {
                Instruction::Assign { dst, value } => {
                    emit_assignment(&mut asm, dst, value, &strings, &label.name)?;
                }
                Instruction::Exit { code } => {
                    asm.push_str("  mov rax, 60\n");
                    asm.push_str(&format!("  mov rdi, {code}\n"));
                    asm.push_str("  syscall\n");
                }
                Instruction::Idiv { divisor } => {
                    let divisor = emit_operand(divisor, &strings, &label.name)?;
                    asm.push_str(&format!("  idiv {divisor}\n"));
                }
                Instruction::Jmp { target } => {
                    asm.push_str(&format!("  jmp {target}\n"));
                }
                Instruction::Let { .. } => {}
                Instruction::Print { parts } => {
                    for part in parts {
                        let string =
                            resolve_print_part(&strings, &mut literal_indexes, &label.name, part)?;

                        emit_print_instruction(&mut asm, string);
                    }
                }
                Instruction::Syscall => asm.push_str("  syscall\n"),
                Instruction::Udiv { divisor } => {
                    let divisor = emit_operand(divisor, &strings, &label.name)?;
                    asm.push_str(&format!("  div {divisor}\n"));
                }
            }
        }

        asm.push('\n');
    }

    Ok(asm)
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
                Instruction::Let { name, value } => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BindingValue, Label};

    #[test]
    fn prints_integer_binding() {
        let program = Program {
            entry: String::from("main"),
            memory: Vec::new(),
            labels: vec![Label {
                name: String::from("main"),
                instructions: vec![
                    Instruction::Let {
                        name: String::from("count"),
                        value: BindingValue::Integer {
                            value: 3,
                            width: None,
                        },
                    },
                    Instruction::Print {
                        parts: vec![PrintPart::Binding(String::from("count"))],
                    },
                    Instruction::Exit { code: 0 },
                ],
            }],
        };

        let asm = emit_x86_64_linux_asm(&program).unwrap();

        assert!(asm.contains(".Lint_main_count:\n  .byte 51\n"));
        assert!(asm.contains("  lea rsi, [rip + .Lint_main_count]\n"));
        assert!(asm.contains("  mov rdx, 1\n"));
    }

    #[test]
    fn uses_integer_binding_as_immediate_operand() {
        let program = Program {
            entry: String::from("main"),
            memory: Vec::new(),
            labels: vec![Label {
                name: String::from("main"),
                instructions: vec![
                    Instruction::Let {
                        name: String::from("count"),
                        value: BindingValue::Integer {
                            value: 3,
                            width: None,
                        },
                    },
                    Instruction::Assign {
                        dst: Operand::Register(String::from("rax")),
                        value: AssignmentValue::Operand(Operand::Ident(String::from("count"))),
                    },
                    Instruction::Exit { code: 0 },
                ],
            }],
        };

        let asm = emit_x86_64_linux_asm(&program).unwrap();

        assert!(asm.contains("  mov rax, 3\n"));
    }

    #[test]
    fn formats_integer_binding() {
        let program = Program {
            entry: String::from("main"),
            memory: Vec::new(),
            labels: vec![Label {
                name: String::from("main"),
                instructions: vec![
                    Instruction::Let {
                        name: String::from("count"),
                        value: BindingValue::Integer {
                            value: 3,
                            width: None,
                        },
                    },
                    Instruction::Print {
                        parts: vec![
                            PrintPart::Literal(String::from("count = ")),
                            PrintPart::Binding(String::from("count")),
                            PrintPart::Literal(String::from("\n")),
                        ],
                    },
                    Instruction::Exit { code: 0 },
                ],
            }],
        };

        let asm = emit_x86_64_linux_asm(&program).unwrap();

        assert!(
            asm.contains(".Lstr_main_literal_1:\n  .byte 99, 111, 117, 110, 116, 32, 61, 32\n")
        );
        assert!(asm.contains(".Lint_main_count:\n  .byte 51\n"));
        assert!(asm.contains(".Lstr_main_literal_2:\n  .byte 10\n"));
    }

    #[test]
    fn rejects_immediate_that_does_not_fit_register_destination() {
        let program = Program {
            entry: String::from("main"),
            memory: Vec::new(),
            labels: vec![Label {
                name: String::from("main"),
                instructions: vec![Instruction::Assign {
                    dst: Operand::Register(String::from("ax")),
                    value: AssignmentValue::Operand(Operand::Immediate(66000)),
                }],
            }],
        };

        let error = emit_x86_64_linux_asm(&program).unwrap_err();

        assert_eq!(
            error,
            "Immediate value 66000 does not fit in 16-bit destination"
        );
    }

    #[test]
    fn rejects_integer_binding_that_does_not_fit_memory_destination() {
        let program = Program {
            entry: String::from("main"),
            memory: Vec::new(),
            labels: vec![Label {
                name: String::from("main"),
                instructions: vec![
                    Instruction::Let {
                        name: String::from("count"),
                        value: BindingValue::Integer {
                            value: 256,
                            width: None,
                        },
                    },
                    Instruction::Assign {
                        dst: Operand::Dereference {
                            address: Address {
                                first: AddressTerm::Register(String::from("rsp")),
                                rest: Vec::new(),
                            },
                            width: Some(MemoryWidth::U8),
                        },
                        value: AssignmentValue::Operand(Operand::Ident(String::from("count"))),
                    },
                ],
            }],
        };

        let error = emit_x86_64_linux_asm(&program).unwrap_err();

        assert_eq!(
            error,
            "Immediate value 256 does not fit in 8-bit destination"
        );
    }

    #[test]
    fn preserves_math_rhs_when_it_is_also_the_destination() {
        let program = Program {
            entry: String::from("main"),
            memory: Vec::new(),
            labels: vec![Label {
                name: String::from("main"),
                instructions: vec![Instruction::Assign {
                    dst: Operand::Register(String::from("rax")),
                    value: AssignmentValue::Binary {
                        op: MathOp::Subtract,
                        lhs: Operand::Register(String::from("rbx")),
                        rhs: Operand::Register(String::from("rax")),
                    },
                }],
            }],
        };

        let asm = emit_x86_64_linux_asm(&program).unwrap();

        assert!(asm.contains("  neg rax\n"));
        assert!(asm.contains("  add rax, rbx\n"));
    }

    #[test]
    fn emits_memory_scalars_and_buffers() {
        let program = Program {
            entry: String::from("main"),
            memory: vec![
                MemoryDeclaration::Scalar {
                    name: String::from("count"),
                    width: MemoryWidth::U16,
                    value: 3,
                },
                MemoryDeclaration::Buffer {
                    name: String::from("buf"),
                    width: MemoryWidth::U8,
                    count: 128,
                },
            ],
            labels: vec![Label {
                name: String::from("main"),
                instructions: vec![Instruction::Exit { code: 0 }],
            }],
        };

        let asm = emit_x86_64_linux_asm(&program).unwrap();

        assert!(asm.contains(".section .data\ncount:\n  .word 3\n\n"));
        assert!(asm.contains(".section .bss\nbuf:\n  .zero 128\n\n"));
    }
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

fn emit_print_instruction(asm: &mut String, string: &StringBinding) {
    asm.push_str("  mov rax, 1\n");
    asm.push_str("  mov rdi, 1\n");
    asm.push_str(&format!("  lea rsi, [rip + {}]\n", string.asm_label));
    asm.push_str(&format!("  mov rdx, {}\n", string.value.len()));
    asm.push_str("  syscall\n");
}

fn emit_binary_instruction(
    asm: &mut String,
    opcode: &str,
    src: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
) -> Result<(), String> {
    validate_binary_operands(opcode, src, dst, strings, label_name)?;

    let src = emit_operand(src, strings, label_name)?;
    let dst = emit_operand(dst, strings, label_name)?;
    asm.push_str(&format!("  {opcode} {dst}, {src}\n"));

    Ok(())
}

fn emit_assignment(
    asm: &mut String,
    dst: &Operand,
    value: &AssignmentValue,
    strings: &StringTable,
    label_name: &str,
) -> Result<(), String> {
    match value {
        AssignmentValue::Operand(src) => emit_copy_instruction(asm, src, dst, strings, label_name),
        AssignmentValue::Binary { op, lhs, rhs } => {
            if !matches!(dst, Operand::Register(_)) {
                return Err(String::from(
                    "Math assignment destination must be a register for now",
                ));
            }

            if lhs == dst {
                let opcode = match op {
                    MathOp::Add => "add",
                    MathOp::Multiply => "imul",
                    MathOp::Subtract => "sub",
                };

                return emit_binary_instruction(asm, opcode, rhs, dst, strings, label_name);
            }

            if rhs == dst {
                match op {
                    MathOp::Add | MathOp::Multiply => {
                        let opcode = match op {
                            MathOp::Add => "add",
                            MathOp::Multiply => "imul",
                            MathOp::Subtract => unreachable!(),
                        };

                        return emit_binary_instruction(asm, opcode, lhs, dst, strings, label_name);
                    }
                    MathOp::Subtract => {
                        let dst_operand = emit_operand(dst, strings, label_name)?;
                        asm.push_str(&format!("  neg {dst_operand}\n"));

                        return emit_binary_instruction(asm, "add", lhs, dst, strings, label_name);
                    }
                }
            }

            {
                emit_copy_instruction(asm, lhs, dst, strings, label_name)?;

                let opcode = match op {
                    MathOp::Add => "add",
                    MathOp::Multiply => "imul",
                    MathOp::Subtract => "sub",
                };

                emit_binary_instruction(asm, opcode, rhs, dst, strings, label_name)
            }
        }
    }
}

fn emit_copy_instruction(
    asm: &mut String,
    src: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
) -> Result<(), String> {
    if let Operand::Pointer(name) = src {
        validate_address_copy_dst(dst)?;

        let dst = emit_operand(dst, strings, label_name)?;
        asm.push_str(&format!("  lea {dst}, [rip + {name}]\n"));

        Ok(())
    } else {
        emit_binary_instruction(asm, "mov", src, dst, strings, label_name)
    }
}

fn validate_binary_operands(
    opcode: &str,
    src: &Operand,
    dst: &Operand,
    strings: &StringTable,
    label_name: &str,
) -> Result<(), String> {
    if matches!(
        dst,
        Operand::Immediate(_) | Operand::Ident(_) | Operand::Pointer(_)
    ) {
        return Err(format!(
            "{opcode} destination must be a register or memory operand"
        ));
    }

    if matches!(src, Operand::Pointer(_)) {
        return Err(format!("{opcode} source cannot be an address-of operand"));
    }

    if matches!(src, Operand::Dereference { .. }) && matches!(dst, Operand::Dereference { .. }) {
        return Err(format!(
            "{opcode} cannot use memory for both source and destination"
        ));
    }

    if opcode == "mov"
        && is_immediate_operand(src, strings, label_name)
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

    if let (Some(src_width), Some(dst_width)) = (operand_width(src), operand_width(dst)) {
        if src_width != dst_width {
            return Err(format!(
                "Cannot use {}-bit source with {}-bit destination",
                src_width.bits(),
                dst_width.bits()
            ));
        }
    }

    if let (Some(value), Some(width)) = (
        immediate_value(src, strings, label_name),
        destination_width(dst),
    ) {
        validate_immediate_range(value, width)?;
    }

    Ok(())
}

fn is_immediate_operand(operand: &Operand, strings: &StringTable, label_name: &str) -> bool {
    match operand {
        Operand::Immediate(_) => true,
        Operand::Ident(name) => strings
            .integers
            .contains_key(&(label_name.to_string(), name.clone())),
        _ => false,
    }
}

fn immediate_value(operand: &Operand, strings: &StringTable, label_name: &str) -> Option<i64> {
    match operand {
        Operand::Immediate(value) => Some(*value),
        Operand::Ident(name) => strings
            .integers
            .get(&(label_name.to_string(), name.clone()))
            .map(|binding| binding.value),
        _ => None,
    }
}

fn destination_width(operand: &Operand) -> Option<Width> {
    match operand {
        Operand::Register(name) => register_width(name),
        Operand::Dereference {
            width: Some(width), ..
        } => Some(memory_width_bits(width)),
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

fn emit_operand(
    operand: &Operand,
    strings: &StringTable,
    label_name: &str,
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
        Operand::Ident(name) => match strings
            .integers
            .get(&(label_name.to_string(), name.clone()))
        {
            Some(binding) => Ok(binding.value.to_string()),
            None => Ok(name.clone()),
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

fn operand_width(operand: &Operand) -> Option<Width> {
    match operand {
        Operand::Register(name) => register_width(name),
        Operand::Dereference { width, .. } => width.as_ref().map(memory_width_bits),
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
