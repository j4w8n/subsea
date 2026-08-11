use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use subsea::ast::{
    Address, AddressTerm, AssignmentTarget, AssignmentValue, BindingValue, CompareOp, Condition,
    FloatMathOp, Instruction, Label, MathOp, MemoryDeclaration, MemoryWidth, Operand, PrintPart,
    Program, ReadSource, StringInitializer, StringProperty,
};
use subsea::codegen::emit_x86_64_linux_asm;

fn s(value: &str) -> String {
    value.to_string()
}

fn ident(value: &str) -> Operand {
    Operand::Ident(s(value))
}

fn ptr(value: &str) -> Operand {
    Operand::Pointer(s(value))
}

fn reg(value: &str) -> Operand {
    Operand::Register(s(value))
}

fn float(value: &str) -> Operand {
    Operand::FloatLiteral(s(value))
}

fn addr_ident(value: &str) -> Address {
    Address {
        first: AddressTerm::Ident(s(value)),
        rest: Vec::new(),
    }
}

fn deref_ident(value: &str, width: Option<MemoryWidth>) -> Operand {
    Operand::Dereference {
        address: addr_ident(value),
        width,
    }
}

fn main_program(mut instructions: Vec<Instruction>) -> Program {
    instructions.push(Instruction::Exit { code: 0 });

    Program {
        entry: s("main"),
        memory: Vec::new(),
        labels: vec![Label {
            name: s("main"),
            instructions,
        }],
    }
}

fn assert_assembles(asm: &str) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let asm_path =
        std::env::temp_dir().join(format!("subsea-codegen-{}-{unique}.s", std::process::id()));
    let object_path = asm_path.with_extension("o");

    std::fs::write(&asm_path, asm).unwrap();

    let output = Command::new("as")
        .arg(&asm_path)
        .arg("-o")
        .arg(&object_path)
        .output()
        .unwrap();

    let _ = std::fs::remove_file(&asm_path);
    let _ = std::fs::remove_file(&object_path);

    assert!(
        output.status.success(),
        "assembler failed:\n{}\nassembly:\n{asm}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn prints_integer_binding() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("count"),
            value: BindingValue::Integer {
                value: 3,
                width: None,
            },
        },
        Instruction::Print {
            parts: vec![PrintPart::Binding(s("count"))],
        },
        Instruction::Exit { code: 0 },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains(".Lint_main_count:\n  .byte 51\n"));
    assert!(asm.contains("  lea rsi, [rip + .Lint_main_count]\n"));
    assert!(asm.contains("  mov rdx, 1\n"));
}

#[test]
fn emits_runtime_integer_print() {
    let program = main_program(vec![Instruction::Print {
        parts: vec![PrintPart::Operand(reg("rax"))],
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, rax\n"));
    assert!(asm.contains(".L.__subsea.main.print_1_loop:\n"));
    assert!(asm.contains("  div rbx\n"));
    assert!(asm.contains("  syscall\n"));
}

#[test]
fn generated_print_labels_do_not_collide_with_local_labels() {
    let program = main_program(vec![
        Instruction::Label {
            name: s(".L.main.print_1_loop"),
        },
        Instruction::Print {
            parts: vec![PrintPart::Operand(reg("rax"))],
        },
        Instruction::Exit { code: 0 },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains(".L.main.print_1_loop:\n"));
    assert!(asm.contains(".L.__subsea.main.print_1_loop:\n"));
    assert_assembles(&asm);
}

#[test]
fn uses_integer_binding_as_immediate_operand() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("count"),
            value: BindingValue::Integer {
                value: 3,
                width: None,
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Operand(ident("count")),
        },
        Instruction::Exit { code: 0 },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, 3\n"));
}

#[test]
fn rejects_string_binding_as_operand() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("message"),
            value: BindingValue::String(s("hi")),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Operand(ident("message")),
        },
    ]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "String binding \"message\" in label \"main\" cannot be used as an operand"
    );
}

#[test]
fn emits_stack_frame_and_stack_assignment() {
    let program = main_program(vec![
        Instruction::Stack {
            name: s("count"),
            width: MemoryWidth::U64,
            value: Operand::Immediate(8),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(ident("count")),
            value: AssignmentValue::Binary {
                op: MathOp::Add,
                lhs: ident("count"),
                rhs: Operand::Immediate(1),
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Operand(ident("count")),
        },
        Instruction::Exit { code: 0 },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("main:\n  push rbp\n  mov rbp, rsp\n  sub rsp, 16\n"));
    assert!(asm.contains("  mov qword ptr [rbp - 8], 8\n"));
    assert!(asm.contains("  add qword ptr [rbp - 8], 1\n"));
    assert!(asm.contains("  mov rax, qword ptr [rbp - 8]\n"));
}

#[test]
fn emits_stack_string_literal_print() {
    let program = main_program(vec![
        Instruction::StackString {
            name: s("message"),
            value: StringInitializer::Literal(s("hello")),
        },
        Instruction::Print {
            parts: vec![PrintPart::Binding(s("message"))],
        },
        Instruction::Exit { code: 0 },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains(".Lstr_main_message:\n  .byte 104, 101, 108, 108, 111\n"));
    assert!(asm.contains("main:\n  push rbp\n  mov rbp, rsp\n  sub rsp, 16\n"));
    assert!(asm.contains("  push r10\n"));
    assert!(asm.contains("  lea r10, [rip + .Lstr_main_message]\n"));
    assert!(asm.contains("  mov qword ptr [rbp - 8], r10\n"));
    assert!(asm.contains("  pop r10\n"));
    assert!(asm.contains("  mov qword ptr [rbp - 16], 5\n"));
    assert!(asm.contains("  mov rsi, qword ptr [rbp - 8]\n"));
    assert!(asm.contains("  mov rdx, qword ptr [rbp - 16]\n"));
}

#[test]
fn emits_stack_string_slice_print() {
    let program = Program {
        entry: s("main"),
        memory: vec![MemoryDeclaration::Buffer {
            name: s("buf"),
            width: MemoryWidth::U8,
            count: 8,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::StackString {
                    name: s("input"),
                    value: StringInitializer::Slice {
                        ptr: ptr("buf"),
                        len: reg("rax"),
                    },
                },
                Instruction::Print {
                    parts: vec![PrintPart::Binding(s("input"))],
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  push r10\n"));
    assert!(asm.contains("  lea r10, [rip + buf]\n"));
    assert!(asm.contains("  mov qword ptr [rbp - 8], r10\n"));
    assert!(asm.contains("  pop r10\n"));
    assert!(asm.contains("  mov qword ptr [rbp - 16], rax\n"));
    assert!(asm.contains("  mov rsi, qword ptr [rbp - 8]\n"));
    assert!(asm.contains("  mov rdx, qword ptr [rbp - 16]\n"));
}

#[test]
fn emits_stack_string_property_loads() {
    let program = main_program(vec![
        Instruction::StackString {
            name: s("message"),
            value: StringInitializer::Literal(s("hello")),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Operand(Operand::StringProperty {
                name: s("message"),
                property: StringProperty::Ptr,
            }),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rbx")),
            value: AssignmentValue::Operand(Operand::StringProperty {
                name: s("message"),
                property: StringProperty::Len,
            }),
        },
        Instruction::Exit { code: 0 },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, qword ptr [rbp - 8]\n"));
    assert!(asm.contains("  mov rbx, qword ptr [rbp - 16]\n"));
}

#[test]
fn rejects_stack_string_property_as_destination() {
    let program = main_program(vec![
        Instruction::StackString {
            name: s("message"),
            value: StringInitializer::Literal(s("hello")),
        },
        Instruction::Pop {
            dst: Operand::StringProperty {
                name: s("message"),
                property: StringProperty::Len,
            },
        },
        Instruction::Exit { code: 0 },
    ]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "pop destination must be a 64-bit register or explicitly 64-bit memory operand"
    );
}

#[test]
fn emits_read_from_stdin() {
    let program = Program {
        entry: s("main"),
        memory: vec![MemoryDeclaration::Buffer {
            name: s("buf"),
            width: MemoryWidth::U8,
            count: 1024,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Read {
                    src: ReadSource::Stdin,
                    dst: ptr("buf"),
                    len: Operand::Immediate(1024),
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rdx, 1024\n"));
    assert!(asm.contains("  lea rsi, [rip + buf]\n"));
    assert!(asm.contains("  mov rdi, 0\n"));
    assert!(asm.contains("  mov rax, 0\n"));
    assert!(asm.contains("  syscall\n"));
}

#[test]
fn emits_stack_cleanup_before_ret() {
    let program = main_program(vec![
        Instruction::Stack {
            name: s("value"),
            width: MemoryWidth::U64,
            value: Operand::Immediate(1),
        },
        Instruction::Ret,
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rsp, rbp\n  pop rbp\n  ret\n"));
}

#[test]
fn rejects_cross_label_jump_from_stack_label() {
    let program = main_program(vec![
        Instruction::Stack {
            name: s("value"),
            width: MemoryWidth::U64,
            value: Operand::Immediate(1),
        },
        Instruction::Jmp {
            target: s("other"),
            condition: None,
        },
    ]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "jmp target \"other\" in function \"main\" must be a local label"
    );
}

#[test]
fn rejects_cross_function_jump_without_stack_frame() {
    let program = Program {
        entry: s("main"),
        memory: Vec::new(),
        labels: vec![
            Label {
                name: s("main"),
                instructions: vec![Instruction::Jmp {
                    target: s("other"),
                    condition: None,
                }],
            },
            Label {
                name: s("other"),
                instructions: vec![Instruction::Ret],
            },
        ],
    };

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "jmp target \"other\" in function \"main\" must be a local label"
    );
}

#[test]
fn rejects_call_to_local_label() {
    let program = main_program(vec![
        Instruction::Label {
            name: s(".L.main.helper"),
        },
        Instruction::Call {
            target: s(".L.main.helper"),
        },
        Instruction::Ret,
    ]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "call target \".L.main.helper\" in function \"main\" must be a top-level function"
    );
}

#[test]
fn formats_integer_binding() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("count"),
            value: BindingValue::Integer {
                value: 3,
                width: None,
            },
        },
        Instruction::Print {
            parts: vec![
                PrintPart::Literal(s("count = ")),
                PrintPart::Binding(s("count")),
                PrintPart::Literal(s("\n")),
            ],
        },
        Instruction::Exit { code: 0 },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains(".Lstr_main_literal_1:\n  .byte 99, 111, 117, 110, 116, 32, 61, 32\n"));
    assert!(asm.contains(".Lint_main_count:\n  .byte 51\n"));
    assert!(asm.contains(".Lstr_main_literal_2:\n  .byte 10\n"));
}

#[test]
fn prints_float_binding_as_literal_text() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("ratio"),
            value: BindingValue::Float {
                value: s("1.5"),
                width: MemoryWidth::F64,
            },
        },
        Instruction::Print {
            parts: vec![PrintPart::Binding(s("ratio"))],
        },
        Instruction::Exit { code: 0 },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains(".Lfloat_main_ratio:\n  .byte 49, 46, 53\n"));
    assert!(asm.contains("  lea rsi, [rip + .Lfloat_main_ratio]\n"));
    assert!(asm.contains("  mov rdx, 3\n"));
}

#[test]
fn rejects_immediate_that_does_not_fit_register_destination() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("ax")),
        value: AssignmentValue::Operand(Operand::Immediate(66000)),
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Immediate value 66000 does not fit in 16-bit destination"
    );
}

#[test]
fn rejects_integer_binding_that_does_not_fit_memory_destination() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("count"),
            value: BindingValue::Integer {
                value: 256,
                width: None,
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Dereference {
                address: Address {
                    first: AddressTerm::Register(s("rsp")),
                    rest: Vec::new(),
                },
                width: Some(MemoryWidth::U8),
            }),
            value: AssignmentValue::Operand(ident("count")),
        },
    ]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Immediate value 256 does not fit in 8-bit destination"
    );
}

#[test]
fn rejects_unsigned_value_for_signed_memory_destination() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(Operand::Dereference {
            address: Address {
                first: AddressTerm::Register(s("rsp")),
                rest: Vec::new(),
            },
            width: Some(MemoryWidth::I8),
        }),
        value: AssignmentValue::Operand(Operand::Immediate(200)),
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Immediate value 200 does not fit in 8-bit destination"
    );
}

#[test]
fn rejects_large_immediate_for_64_bit_memory_destination() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(Operand::Dereference {
            address: Address {
                first: AddressTerm::Register(s("rsp")),
                rest: Vec::new(),
            },
            width: Some(MemoryWidth::U64),
        }),
        value: AssignmentValue::Operand(Operand::Immediate(2_147_483_648)),
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Immediate value 2147483648 cannot be encoded directly into a 64-bit memory destination; move it through a 64-bit register first"
    );
}

#[test]
fn preserves_math_rhs_when_it_is_also_the_destination() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Binary {
            op: MathOp::Subtract,
            lhs: reg("rbx"),
            rhs: reg("rax"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  neg rax\n"));
    assert!(asm.contains("  add rax, rbx\n"));
}

#[test]
fn rejects_binary_assignment_when_destination_is_used_in_rhs_address() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Binary {
            op: MathOp::Add,
            lhs: reg("rbx"),
            rhs: Operand::Dereference {
                address: Address {
                    first: AddressTerm::Register(s("rax")),
                    rest: Vec::new(),
                },
                width: Some(MemoryWidth::U64),
            },
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Binary assignment destination rax cannot be used in the right operand address"
    );
}

#[test]
fn emits_unsigned_widened_multiply() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair {
            high: s("rdx"),
            low: s("rax"),
        },
        value: AssignmentValue::WideMultiply {
            signed: false,
            lhs: reg("rbx"),
            rhs: reg("rcx"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, rbx\n"));
    assert!(asm.contains("  mul rcx\n"));
}

#[test]
fn rejects_address_of_into_non_64_bit_register() {
    let program = Program {
        entry: s("main"),
        memory: vec![MemoryDeclaration::Buffer {
            name: s("buf"),
            width: MemoryWidth::U8,
            count: 8,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![Instruction::Assign {
                dst: AssignmentTarget::Operand(reg("eax")),
                value: AssignmentValue::Operand(ptr("buf")),
            }],
        }],
    };

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Address-of labels can only be copied into 64-bit registers, found 32-bit register"
    );
}

#[test]
fn emits_signed_widened_multiply() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair {
            high: s("rdx"),
            low: s("rax"),
        },
        value: AssignmentValue::WideMultiply {
            signed: true,
            lhs: reg("rbx"),
            rhs: reg("rcx"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, rbx\n"));
    assert!(asm.contains("  imul rcx\n"));
}

#[test]
fn rejects_non_rdx_rax_widened_multiply_destination() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair {
            high: s("r9"),
            low: s("r8"),
        },
        value: AssignmentValue::WideMultiply {
            signed: false,
            lhs: reg("rbx"),
            rhs: reg("rcx"),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Widened multiply destination must be rdx:rax, found r9:r8"
    );
}

#[test]
fn rejects_non_rdx_rax_widened_divide_destination() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair {
            high: s("r9"),
            low: s("r8"),
        },
        value: AssignmentValue::WideDivide {
            signed: false,
            lhs: reg("rbx"),
            rhs: reg("rcx"),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Widened division destination must be rdx:rax, found r9:r8"
    );
}

#[test]
fn rejects_immediate_widened_multiply_rhs() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair {
            high: s("rdx"),
            low: s("rax"),
        },
        value: AssignmentValue::WideMultiply {
            signed: false,
            lhs: reg("rbx"),
            rhs: Operand::Immediate(2),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Widened multiply right operand cannot be an immediate value"
    );
}

#[test]
fn rejects_immediate_widened_multiply_lhs() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair {
            high: s("rdx"),
            low: s("rax"),
        },
        value: AssignmentValue::WideMultiply {
            signed: false,
            lhs: Operand::Immediate(10),
            rhs: reg("rcx"),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Widened multiply left operand cannot be an immediate value"
    );
}

#[test]
fn rejects_widened_multiply_rhs_that_uses_rax() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair {
            high: s("rdx"),
            low: s("rax"),
        },
        value: AssignmentValue::WideMultiply {
            signed: false,
            lhs: reg("rbx"),
            rhs: reg("rax"),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Widened multiply right operand cannot use rax because rax is overwritten before the operation"
    );
}

#[test]
fn emits_unsigned_widened_divide() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair {
            high: s("rdx"),
            low: s("rax"),
        },
        value: AssignmentValue::WideDivide {
            signed: false,
            lhs: reg("rbx"),
            rhs: reg("rcx"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, rbx\n"));
    assert!(asm.contains("  xor rdx, rdx\n"));
    assert!(asm.contains("  div rcx\n"));
}

#[test]
fn emits_signed_widened_divide() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair {
            high: s("rdx"),
            low: s("rax"),
        },
        value: AssignmentValue::WideDivide {
            signed: true,
            lhs: reg("rbx"),
            rhs: reg("rcx"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, rbx\n"));
    assert!(asm.contains("  cqo\n"));
    assert!(asm.contains("  idiv rcx\n"));
}

#[test]
fn rejects_immediate_widened_divide_rhs() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair {
            high: s("rdx"),
            low: s("rax"),
        },
        value: AssignmentValue::WideDivide {
            signed: false,
            lhs: reg("rbx"),
            rhs: Operand::Immediate(2),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Widened division right operand cannot be an immediate value"
    );
}

#[test]
fn rejects_widened_divide_rhs_that_uses_rdx() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair {
            high: s("rdx"),
            low: s("rax"),
        },
        value: AssignmentValue::WideDivide {
            signed: false,
            lhs: reg("rbx"),
            rhs: reg("rdx"),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Widened division right operand cannot use rdx because rdx is overwritten before the operation"
    );
}

#[test]
fn emits_call_and_ret() {
    let program = Program {
        entry: s("main"),
        memory: Vec::new(),
        labels: vec![
            Label {
                name: s("main"),
                instructions: vec![
                    Instruction::Call {
                        target: s("helper"),
                    },
                    Instruction::Ret,
                ],
            },
            Label {
                name: s("helper"),
                instructions: vec![Instruction::Ret],
            },
        ],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  call helper\n"));
    assert!(asm.contains("  ret\n"));
}

#[test]
fn emits_push_and_pop() {
    let program = main_program(vec![
        Instruction::Push { src: reg("rax") },
        Instruction::Push {
            src: Operand::Immediate(10),
        },
        Instruction::Pop { dst: reg("rbx") },
        Instruction::Pop { dst: reg("rax") },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  push rax\n"));
    assert!(asm.contains("  push 10\n"));
    assert!(asm.contains("  pop rbx\n"));
    assert!(asm.contains("  pop rax\n"));
}

#[test]
fn rejects_ret_with_unbalanced_manual_stack() {
    let program = main_program(vec![
        Instruction::Push { src: reg("rax") },
        Instruction::Ret,
    ]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Function \"main\" cannot ret with unbalanced manual stack depth 1. Pop pushed values before the function ends, or use `exit` if this path terminates the process."
    );
}

#[test]
fn rejects_local_label_with_conflicting_manual_stack_depths() {
    let program = main_program(vec![
        Instruction::Jmp {
            target: s(".L.main.join"),
            condition: Some(Condition {
                lhs: reg("rax"),
                op: CompareOp::Equal,
                rhs: Operand::Immediate(0),
            }),
        },
        Instruction::Push { src: reg("rax") },
        Instruction::Label {
            name: s(".L.main.join"),
        },
        Instruction::Exit { code: 0 },
    ]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Function \"main\" reaches instruction 2 with conflicting stack depths 0 and 1"
    );
}

#[test]
fn emits_inline_label() {
    let program = main_program(vec![
        Instruction::Label {
            name: s(".L.main.loop"),
        },
        Instruction::Jmp {
            target: s(".L.main.loop"),
            condition: None,
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("main:\n.L.main.loop:\n  jmp .L.main.loop\n"));
}

#[test]
fn emits_signed_conditional_jump() {
    let program = main_program(vec![
        Instruction::Jmp {
            target: s(".L.main.done"),
            condition: Some(Condition {
                lhs: reg("rax"),
                op: CompareOp::SignedLess,
                rhs: Operand::Immediate(0),
            }),
        },
        Instruction::Label {
            name: s(".L.main.done"),
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  cmp rax, 0\n  jl .L.main.done\n"));
}

#[test]
fn emits_unsigned_conditional_jump() {
    let program = main_program(vec![
        Instruction::Label {
            name: s(".L.main.loop"),
        },
        Instruction::Jmp {
            target: s(".L.main.loop"),
            condition: Some(Condition {
                lhs: reg("rcx"),
                op: CompareOp::UnsignedLess,
                rhs: reg("rbx"),
            }),
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  cmp rcx, rbx\n  jb .L.main.loop\n"));
}

#[test]
fn rejects_function_fallthrough() {
    let program = Program {
        entry: s("main"),
        memory: Vec::new(),
        labels: vec![
            Label {
                name: s("main"),
                instructions: vec![Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("rax")),
                    value: AssignmentValue::Operand(Operand::Immediate(1)),
                }],
            },
            Label {
                name: s("next"),
                instructions: vec![Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("rbx")),
                    value: AssignmentValue::Operand(Operand::Immediate(2)),
                }],
            },
        ],
    };

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Function \"main\" can fall through. End this path with `ret`, `exit`, or an unconditional local `jmp` to code that does."
    );
}

#[test]
fn rejects_non_64_bit_push_register() {
    let program = main_program(vec![Instruction::Push { src: reg("eax") }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(error, "push source must be 64-bit, found 32-bit register");
}

#[test]
fn rejects_pop_immediate_destination() {
    let program = main_program(vec![Instruction::Pop {
        dst: Operand::Immediate(10),
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "pop destination must be a 64-bit register or explicitly 64-bit memory operand"
    );
}

#[test]
fn rejects_pop_memory_without_width() {
    let program = main_program(vec![Instruction::Pop {
        dst: Operand::Dereference {
            address: Address {
                first: AddressTerm::Register(s("rsp")),
                rest: Vec::new(),
            },
            width: None,
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "pop destination memory operand requires an explicit 64-bit width"
    );
}

#[test]
fn allows_stack_label_to_end_with_explicit_exit_syscall() {
    let program = main_program(vec![
        Instruction::Stack {
            name: s("value"),
            width: MemoryWidth::U64,
            value: Operand::Immediate(1),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Operand(Operand::Immediate(60)),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rdi")),
            value: AssignmentValue::Operand(Operand::Immediate(0)),
        },
        Instruction::Syscall,
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  syscall\n"));
}

#[test]
fn allows_stack_label_to_end_with_exit_syscall_after_extra_setup() {
    let program = main_program(vec![
        Instruction::Stack {
            name: s("value"),
            width: MemoryWidth::U64,
            value: Operand::Immediate(1),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Operand(Operand::Immediate(60)),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rbx")),
            value: AssignmentValue::Operand(Operand::Immediate(123)),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rdi")),
            value: AssignmentValue::Operand(Operand::Immediate(0)),
        },
        Instruction::Syscall,
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  syscall\n"));
}

#[test]
fn rejects_printing_high_byte_register() {
    let program = main_program(vec![Instruction::Print {
        parts: vec![PrintPart::Operand(reg("ah"))],
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "print operand cannot use high-byte registers ah, bh, ch, or dh"
    );
}

#[test]
fn rejects_high_byte_register_with_extended_register() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("r8b")),
        value: AssignmentValue::Operand(reg("ah")),
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "mov cannot combine high-byte registers ah, bh, ch, or dh with extended registers"
    );
}

#[test]
fn emits_memory_scalars_and_buffers() {
    let program = Program {
        entry: s("main"),
        memory: vec![
            MemoryDeclaration::Scalar {
                name: s("count"),
                width: MemoryWidth::U16,
                value: 3,
            },
            MemoryDeclaration::Buffer {
                name: s("buf"),
                width: MemoryWidth::U8,
                count: 128,
            },
        ],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![Instruction::Exit { code: 0 }],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains(".section .data\ncount:\n  .word 3\n\n"));
    assert!(asm.contains(".section .bss\nbuf:\n  .zero 128\n\n"));
}

#[test]
fn emits_float_memory_scalars() {
    let program = Program {
        entry: s("main"),
        memory: vec![
            MemoryDeclaration::FloatScalar {
                name: s("single"),
                width: MemoryWidth::F32,
                value: s("1.5"),
            },
            MemoryDeclaration::FloatScalar {
                name: s("double"),
                width: MemoryWidth::F64,
                value: s("-2.25"),
            },
        ],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![Instruction::Exit { code: 0 }],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("single:\n  .float 1.5\n"));
    assert!(asm.contains("double:\n  .double -2.25\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_xmm_float_loads_and_stores() {
    let program = Program {
        entry: s("main"),
        memory: vec![
            MemoryDeclaration::FloatScalar {
                name: s("single"),
                width: MemoryWidth::F32,
                value: s("1.5"),
            },
            MemoryDeclaration::FloatScalar {
                name: s("double"),
                width: MemoryWidth::F64,
                value: s("2.25"),
            },
        ],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("xmm0")),
                    value: AssignmentValue::Operand(deref_ident("single", Some(MemoryWidth::F32))),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("xmm1")),
                    value: AssignmentValue::Operand(deref_ident("double", Some(MemoryWidth::F64))),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(deref_ident("single", Some(MemoryWidth::F32))),
                    value: AssignmentValue::Operand(reg("xmm0")),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(deref_ident("double", Some(MemoryWidth::F64))),
                    value: AssignmentValue::Operand(reg("xmm1")),
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  movss xmm0, dword ptr [single]\n"));
    assert!(asm.contains("  movsd xmm1, qword ptr [double]\n"));
    assert!(asm.contains("  movss dword ptr [single], xmm0\n"));
    assert!(asm.contains("  movsd qword ptr [double], xmm1\n"));
    assert_assembles(&asm);
}

#[test]
fn infers_memory_width_from_declared_base() {
    let program = Program {
        entry: s("main"),
        memory: vec![MemoryDeclaration::Buffer {
            name: s("buf"),
            width: MemoryWidth::U8,
            count: 8,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(deref_ident("buf", None)),
                    value: AssignmentValue::Operand(Operand::Immediate(72)),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Dereference {
                        address: Address {
                            first: AddressTerm::Ident(s("buf")),
                            rest: vec![(
                                subsea::ast::AddressOperator::Add,
                                AddressTerm::Immediate(1),
                            )],
                        },
                        width: None,
                    }),
                    value: AssignmentValue::Operand(Operand::Immediate(105)),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("al")),
                    value: AssignmentValue::Operand(deref_ident("buf", None)),
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov byte ptr [buf], 72\n"));
    assert!(asm.contains("  mov byte ptr [buf + 1], 105\n"));
    assert!(asm.contains("  mov al, byte ptr [buf]\n"));
    assert_assembles(&asm);
}

#[test]
fn rejects_inferred_memory_width_mismatch() {
    let program = Program {
        entry: s("main"),
        memory: vec![MemoryDeclaration::Buffer {
            name: s("buf"),
            width: MemoryWidth::U8,
            count: 8,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![Instruction::Assign {
                dst: AssignmentTarget::Operand(reg("rax")),
                value: AssignmentValue::Operand(deref_ident("buf", None)),
            }],
        }],
    };

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(error, "Cannot use 8-bit source with 64-bit destination");
}

#[test]
fn rejects_untyped_pointer_memory_immediate_store() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(Operand::Dereference {
            address: Address {
                first: AddressTerm::Register(s("rax")),
                rest: Vec::new(),
            },
            width: None,
        }),
        value: AssignmentValue::Operand(Operand::Immediate(1)),
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Cannot assign an immediate value directly into memory without an explicit width"
    );
}

#[test]
fn rejects_negative_immediate_for_inferred_unsigned_memory() {
    let program = Program {
        entry: s("main"),
        memory: vec![MemoryDeclaration::Buffer {
            name: s("buf"),
            width: MemoryWidth::U8,
            count: 8,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![Instruction::Assign {
                dst: AssignmentTarget::Operand(deref_ident("buf", None)),
                value: AssignmentValue::Operand(Operand::Immediate(-1)),
            }],
        }],
    };

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Immediate value -1 does not fit in 8-bit destination"
    );
}

#[test]
fn rejects_integer_register_float_memory_load() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Operand(deref_ident("double", Some(MemoryWidth::F64))),
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Floating-point memory operands require an XMM register source or destination"
    );
}

#[test]
fn rejects_xmm_integer_memory_load() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("xmm0")),
        value: AssignmentValue::Operand(Operand::Dereference {
            address: Address {
                first: AddressTerm::Register(s("rax")),
                rest: Vec::new(),
            },
            width: Some(MemoryWidth::U64),
        }),
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "XMM moves require one XMM register and one explicitly f32 or f64 memory operand"
    );
}

#[test]
fn rejects_xmm_register_to_register_move_for_now() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("xmm0")),
        value: AssignmentValue::Operand(reg("xmm1")),
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "XMM moves require one XMM register and one explicitly f32 or f64 memory operand"
    );
}

#[test]
fn emits_xmm_float_register_arithmetic() {
    let program = main_program(vec![
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm0")),
            value: AssignmentValue::FloatBinary {
                width: MemoryWidth::F32,
                op: FloatMathOp::Add,
                lhs: reg("xmm0"),
                rhs: reg("xmm1"),
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm2")),
            value: AssignmentValue::FloatBinary {
                width: MemoryWidth::F64,
                op: FloatMathOp::Multiply,
                lhs: reg("xmm3"),
                rhs: reg("xmm4"),
            },
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  addss xmm0, xmm1\n"));
    assert!(asm.contains("  movsd xmm2, xmm3\n"));
    assert!(asm.contains("  mulsd xmm2, xmm4\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_xmm_float_memory_arithmetic() {
    let program = Program {
        entry: s("main"),
        memory: vec![
            MemoryDeclaration::FloatScalar {
                name: s("single"),
                width: MemoryWidth::F32,
                value: s("1.5"),
            },
            MemoryDeclaration::FloatScalar {
                name: s("double"),
                width: MemoryWidth::F64,
                value: s("2.25"),
            },
        ],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("xmm0")),
                    value: AssignmentValue::FloatBinary {
                        width: MemoryWidth::F32,
                        op: FloatMathOp::Subtract,
                        lhs: reg("xmm0"),
                        rhs: deref_ident("single", Some(MemoryWidth::F32)),
                    },
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("xmm1")),
                    value: AssignmentValue::FloatBinary {
                        width: MemoryWidth::F64,
                        op: FloatMathOp::Divide,
                        lhs: deref_ident("double", Some(MemoryWidth::F64)),
                        rhs: reg("xmm2"),
                    },
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  subss xmm0, dword ptr [single]\n"));
    assert!(asm.contains("  movsd xmm1, qword ptr [double]\n"));
    assert!(asm.contains("  divsd xmm1, xmm2\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_float_const_and_literal_arithmetic_operands() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("ratio"),
            value: BindingValue::Float {
                value: s("1.5"),
                width: MemoryWidth::F64,
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm0")),
            value: AssignmentValue::FloatBinary {
                width: MemoryWidth::F64,
                op: FloatMathOp::Add,
                lhs: reg("xmm0"),
                rhs: ident("ratio"),
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm0")),
            value: AssignmentValue::FloatBinary {
                width: MemoryWidth::F64,
                op: FloatMathOp::Multiply,
                lhs: reg("xmm0"),
                rhs: float("2.0"),
            },
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains(".Lfloatval_main_ratio:\n  .double 1.5\n"));
    assert!(asm.contains(".Lfloatlit_main_2:\n  .double 2.0\n"));
    assert!(asm.contains("  addsd xmm0, qword ptr [rip + .Lfloatval_main_ratio]\n"));
    assert!(asm.contains("  mulsd xmm0, qword ptr [rip + .Lfloatlit_main_2]\n"));
    assert_assembles(&asm);
}

#[test]
fn infers_plain_float_arithmetic_width_from_const() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("ratio"),
            value: BindingValue::Float {
                value: s("1.5"),
                width: MemoryWidth::F64,
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm0")),
            value: AssignmentValue::Binary {
                op: MathOp::Add,
                lhs: reg("xmm0"),
                rhs: ident("ratio"),
            },
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  addsd xmm0, qword ptr [rip + .Lfloatval_main_ratio]\n"));
    assert_assembles(&asm);
}

#[test]
fn infers_plain_float_arithmetic_width_from_memory() {
    let program = Program {
        entry: s("main"),
        memory: vec![MemoryDeclaration::FloatScalar {
            name: s("ratio"),
            width: MemoryWidth::F32,
            value: s("1.5"),
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("xmm0")),
                    value: AssignmentValue::Binary {
                        op: MathOp::Multiply,
                        lhs: reg("xmm0"),
                        rhs: deref_ident("ratio", None),
                    },
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mulss xmm0, dword ptr [ratio]\n"));
    assert_assembles(&asm);
}

#[test]
fn keeps_plain_xmm_literal_arithmetic_ambiguous() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("xmm0")),
        value: AssignmentValue::Binary {
            op: MathOp::Multiply,
            lhs: reg("xmm0"),
            rhs: float("2.0"),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Floating-point arithmetic width is ambiguous; use f32* or f64*"
    );
}

#[test]
fn emits_stack_float_load_store_and_initializer() {
    let program = main_program(vec![
        Instruction::Stack {
            name: s("ratio"),
            width: MemoryWidth::F64,
            value: float("1.5"),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm0")),
            value: AssignmentValue::Operand(ident("ratio")),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(ident("ratio")),
            value: AssignmentValue::Operand(reg("xmm0")),
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains(".Lfloatlit_main_1:\n  .double 1.5\n"));
    assert!(asm.contains("  mov rax, qword ptr [rip + .Lfloatlit_main_1]\n"));
    assert!(asm.contains("  mov qword ptr [rbp - 8], rax\n"));
    assert!(asm.contains("  movsd xmm0, qword ptr [rbp - 8]\n"));
    assert!(asm.contains("  movsd qword ptr [rbp - 8], xmm0\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_ordered_float_conditional_jump() {
    let program = main_program(vec![
        Instruction::Jmp {
            target: s(".L.main.done"),
            condition: Some(Condition {
                lhs: reg("xmm0"),
                op: CompareOp::FloatLess(MemoryWidth::F64),
                rhs: float("1.5"),
            }),
        },
        Instruction::Label {
            name: s(".L.main.done"),
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains(".Lfloatlit_main_1:\n  .double 1.5\n"));
    assert!(asm.contains("  ucomisd xmm0, qword ptr [rip + .Lfloatlit_main_1]\n"));
    assert!(asm.contains("  jp .L.__subsea.main.fcmp_1_ordered\n"));
    assert!(asm.contains("  jb .L.main.done\n"));
    assert_assembles(&asm);
}

#[test]
fn infers_plain_float_comparison_width_from_const() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("limit"),
            value: BindingValue::Float {
                value: s("1.5"),
                width: MemoryWidth::F64,
            },
        },
        Instruction::Jmp {
            target: s(".L.main.done"),
            condition: Some(Condition {
                lhs: reg("xmm0"),
                op: CompareOp::Less,
                rhs: ident("limit"),
            }),
        },
        Instruction::Label {
            name: s(".L.main.done"),
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  ucomisd xmm0, qword ptr [rip + .Lfloatval_main_limit]\n"));
    assert!(asm.contains("  jb .L.main.done\n"));
    assert_assembles(&asm);
}

#[test]
fn infers_plain_float_comparison_width_from_memory() {
    let program = Program {
        entry: s("main"),
        memory: vec![MemoryDeclaration::FloatScalar {
            name: s("limit"),
            width: MemoryWidth::F32,
            value: s("1.5"),
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Jmp {
                    target: s(".L.main.done"),
                    condition: Some(Condition {
                        lhs: reg("xmm0"),
                        op: CompareOp::LessEqual,
                        rhs: deref_ident("limit", None),
                    }),
                },
                Instruction::Label {
                    name: s(".L.main.done"),
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  ucomiss xmm0, dword ptr [limit]\n"));
    assert!(asm.contains("  jbe .L.main.done\n"));
    assert_assembles(&asm);
}

#[test]
fn rejects_plain_integer_ordered_comparison_without_signedness() {
    let program = main_program(vec![
        Instruction::Jmp {
            target: s(".L.main.done"),
            condition: Some(Condition {
                lhs: reg("rax"),
                op: CompareOp::Less,
                rhs: reg("rbx"),
            }),
        },
        Instruction::Label {
            name: s(".L.main.done"),
        },
    ]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Comparison '<' must specify signedness; use i< or u<"
    );
}

#[test]
fn rejects_float_literal_without_float_width_context() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Operand(float("1.5")),
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(error, "mov cannot use floating-point literal operands");
}

#[test]
fn rejects_float_arithmetic_to_integer_register() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::FloatBinary {
            width: MemoryWidth::F64,
            op: FloatMathOp::Add,
            lhs: reg("xmm0"),
            rhs: reg("xmm1"),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Floating-point arithmetic destination must be an XMM register"
    );
}

#[test]
fn rejects_float_arithmetic_width_mismatch() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("xmm0")),
        value: AssignmentValue::FloatBinary {
            width: MemoryWidth::F64,
            op: FloatMathOp::Add,
            lhs: reg("xmm0"),
            rhs: Operand::Dereference {
                address: Address {
                    first: AddressTerm::Register(s("rax")),
                    rest: Vec::new(),
                },
                width: Some(MemoryWidth::F32),
            },
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Floating-point arithmetic right operand width must match the floating-point operator width"
    );
}

#[test]
fn generated_edge_case_assembly_assembles() {
    let program = Program {
        entry: s("main"),
        memory: vec![MemoryDeclaration::Buffer {
            name: s("buf"),
            width: MemoryWidth::U8,
            count: 16,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("rax")),
                    value: AssignmentValue::Operand(ptr("buf")),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Dereference {
                        address: Address {
                            first: AddressTerm::Register(s("rax")),
                            rest: Vec::new(),
                        },
                        width: Some(MemoryWidth::U64),
                    }),
                    value: AssignmentValue::Operand(Operand::Immediate(i32::MAX as i128)),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::RegisterPair {
                        high: s("rdx"),
                        low: s("rax"),
                    },
                    value: AssignmentValue::WideMultiply {
                        signed: false,
                        lhs: reg("rbx"),
                        rhs: reg("rcx"),
                    },
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert_assembles(&asm);
}
