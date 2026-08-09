use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use subsea::ast::{
    Address, AddressTerm, AssignmentTarget, AssignmentValue, BindingValue, CompareOp, Condition,
    FloatMathOp, Instruction, Label, MathOp, MemoryDeclaration, MemoryWidth, Operand, PrintPart,
    Program, ReadSource, StringInitializer,
};
use subsea::codegen::emit_x86_64_linux_asm;

fn main_program(instructions: Vec<Instruction>) -> Program {
    Program {
        entry: String::from("main"),
        memory: Vec::new(),
        labels: vec![Label {
            name: String::from("main"),
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
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains(".Lint_main_count:\n  .byte 51\n"));
    assert!(asm.contains("  lea rsi, [rip + .Lint_main_count]\n"));
    assert!(asm.contains("  mov rdx, 1\n"));
}

#[test]
fn emits_runtime_integer_print() {
    let program = main_program(vec![Instruction::Print {
        parts: vec![PrintPart::Operand(Operand::Register(String::from("rax")))],
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
            name: String::from(".L.main.print_1_loop"),
        },
        Instruction::Print {
            parts: vec![PrintPart::Operand(Operand::Register(String::from("rax")))],
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
            name: String::from("count"),
            value: BindingValue::Integer {
                value: 3,
                width: None,
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Register(String::from("rax"))),
            value: AssignmentValue::Operand(Operand::Ident(String::from("count"))),
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
            name: String::from("message"),
            value: BindingValue::String(String::from("hi")),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Register(String::from("rax"))),
            value: AssignmentValue::Operand(Operand::Ident(String::from("message"))),
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
            name: String::from("count"),
            width: MemoryWidth::U64,
            value: Operand::Immediate(8),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Ident(String::from("count"))),
            value: AssignmentValue::Binary {
                op: MathOp::Add,
                lhs: Operand::Ident(String::from("count")),
                rhs: Operand::Immediate(1),
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Register(String::from("rax"))),
            value: AssignmentValue::Operand(Operand::Ident(String::from("count"))),
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
            name: String::from("message"),
            value: StringInitializer::Literal(String::from("hello")),
        },
        Instruction::Print {
            parts: vec![PrintPart::Binding(String::from("message"))],
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
        entry: String::from("main"),
        memory: vec![MemoryDeclaration::Buffer {
            name: String::from("buf"),
            width: MemoryWidth::U8,
            count: 8,
        }],
        labels: vec![Label {
            name: String::from("main"),
            instructions: vec![
                Instruction::StackString {
                    name: String::from("input"),
                    value: StringInitializer::Slice {
                        ptr: Operand::Pointer(String::from("buf")),
                        len: Operand::Register(String::from("rax")),
                    },
                },
                Instruction::Print {
                    parts: vec![PrintPart::Binding(String::from("input"))],
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
fn emits_read_from_stdin() {
    let program = Program {
        entry: String::from("main"),
        memory: vec![MemoryDeclaration::Buffer {
            name: String::from("buf"),
            width: MemoryWidth::U8,
            count: 1024,
        }],
        labels: vec![Label {
            name: String::from("main"),
            instructions: vec![
                Instruction::Read {
                    src: ReadSource::Stdin,
                    dst: Operand::Pointer(String::from("buf")),
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
            name: String::from("value"),
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
            name: String::from("value"),
            width: MemoryWidth::U64,
            value: Operand::Immediate(1),
        },
        Instruction::Jmp {
            target: String::from("other"),
            condition: None,
        },
    ]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Label \"main\" declares stack variables and cannot jump to top-level label \"other\""
    );
}

#[test]
fn formats_integer_binding() {
    let program = main_program(vec![
        Instruction::Const {
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
            name: String::from("ratio"),
            value: BindingValue::Float {
                value: String::from("1.5"),
                width: MemoryWidth::F64,
            },
        },
        Instruction::Print {
            parts: vec![PrintPart::Binding(String::from("ratio"))],
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
        dst: AssignmentTarget::Operand(Operand::Register(String::from("ax"))),
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
            name: String::from("count"),
            value: BindingValue::Integer {
                value: 256,
                width: None,
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Dereference {
                address: Address {
                    first: AddressTerm::Register(String::from("rsp")),
                    rest: Vec::new(),
                },
                width: Some(MemoryWidth::U8),
            }),
            value: AssignmentValue::Operand(Operand::Ident(String::from("count"))),
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
                first: AddressTerm::Register(String::from("rsp")),
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
                first: AddressTerm::Register(String::from("rsp")),
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
        dst: AssignmentTarget::Operand(Operand::Register(String::from("rax"))),
        value: AssignmentValue::Binary {
            op: MathOp::Subtract,
            lhs: Operand::Register(String::from("rbx")),
            rhs: Operand::Register(String::from("rax")),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  neg rax\n"));
    assert!(asm.contains("  add rax, rbx\n"));
}

#[test]
fn rejects_binary_assignment_when_destination_is_used_in_rhs_address() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(Operand::Register(String::from("rax"))),
        value: AssignmentValue::Binary {
            op: MathOp::Add,
            lhs: Operand::Register(String::from("rbx")),
            rhs: Operand::Dereference {
                address: Address {
                    first: AddressTerm::Register(String::from("rax")),
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
            high: String::from("rdx"),
            low: String::from("rax"),
        },
        value: AssignmentValue::WideMultiply {
            signed: false,
            lhs: Operand::Register(String::from("rbx")),
            rhs: Operand::Register(String::from("rcx")),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, rbx\n"));
    assert!(asm.contains("  mul rcx\n"));
}

#[test]
fn rejects_address_of_into_non_64_bit_register() {
    let program = Program {
        entry: String::from("main"),
        memory: vec![MemoryDeclaration::Buffer {
            name: String::from("buf"),
            width: MemoryWidth::U8,
            count: 8,
        }],
        labels: vec![Label {
            name: String::from("main"),
            instructions: vec![Instruction::Assign {
                dst: AssignmentTarget::Operand(Operand::Register(String::from("eax"))),
                value: AssignmentValue::Operand(Operand::Pointer(String::from("buf"))),
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
            high: String::from("rdx"),
            low: String::from("rax"),
        },
        value: AssignmentValue::WideMultiply {
            signed: true,
            lhs: Operand::Register(String::from("rbx")),
            rhs: Operand::Register(String::from("rcx")),
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
            high: String::from("r9"),
            low: String::from("r8"),
        },
        value: AssignmentValue::WideMultiply {
            signed: false,
            lhs: Operand::Register(String::from("rbx")),
            rhs: Operand::Register(String::from("rcx")),
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
            high: String::from("r9"),
            low: String::from("r8"),
        },
        value: AssignmentValue::WideDivide {
            signed: false,
            lhs: Operand::Register(String::from("rbx")),
            rhs: Operand::Register(String::from("rcx")),
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
            high: String::from("rdx"),
            low: String::from("rax"),
        },
        value: AssignmentValue::WideMultiply {
            signed: false,
            lhs: Operand::Register(String::from("rbx")),
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
            high: String::from("rdx"),
            low: String::from("rax"),
        },
        value: AssignmentValue::WideMultiply {
            signed: false,
            lhs: Operand::Immediate(10),
            rhs: Operand::Register(String::from("rcx")),
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
            high: String::from("rdx"),
            low: String::from("rax"),
        },
        value: AssignmentValue::WideMultiply {
            signed: false,
            lhs: Operand::Register(String::from("rbx")),
            rhs: Operand::Register(String::from("rax")),
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
            high: String::from("rdx"),
            low: String::from("rax"),
        },
        value: AssignmentValue::WideDivide {
            signed: false,
            lhs: Operand::Register(String::from("rbx")),
            rhs: Operand::Register(String::from("rcx")),
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
            high: String::from("rdx"),
            low: String::from("rax"),
        },
        value: AssignmentValue::WideDivide {
            signed: true,
            lhs: Operand::Register(String::from("rbx")),
            rhs: Operand::Register(String::from("rcx")),
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
            high: String::from("rdx"),
            low: String::from("rax"),
        },
        value: AssignmentValue::WideDivide {
            signed: false,
            lhs: Operand::Register(String::from("rbx")),
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
            high: String::from("rdx"),
            low: String::from("rax"),
        },
        value: AssignmentValue::WideDivide {
            signed: false,
            lhs: Operand::Register(String::from("rbx")),
            rhs: Operand::Register(String::from("rdx")),
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
    let program = main_program(vec![
        Instruction::Call {
            target: String::from("helper"),
        },
        Instruction::Ret,
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  call helper\n"));
    assert!(asm.contains("  ret\n"));
}

#[test]
fn emits_push_and_pop() {
    let program = main_program(vec![
        Instruction::Push {
            src: Operand::Register(String::from("rax")),
        },
        Instruction::Push {
            src: Operand::Immediate(10),
        },
        Instruction::Pop {
            dst: Operand::Register(String::from("rbx")),
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  push rax\n"));
    assert!(asm.contains("  push 10\n"));
    assert!(asm.contains("  pop rbx\n"));
}

#[test]
fn emits_inline_label() {
    let program = main_program(vec![
        Instruction::Label {
            name: String::from(".L.main.loop"),
        },
        Instruction::Jmp {
            target: String::from(".L.main.loop"),
            condition: None,
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("main:\n.L.main.loop:\n  jmp .L.main.loop\n"));
}

#[test]
fn emits_signed_conditional_jump() {
    let program = main_program(vec![Instruction::Jmp {
        target: String::from("done"),
        condition: Some(Condition {
            lhs: Operand::Register(String::from("rax")),
            op: CompareOp::SignedLess,
            rhs: Operand::Immediate(0),
        }),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  cmp rax, 0\n  jl done\n"));
}

#[test]
fn emits_unsigned_conditional_jump() {
    let program = main_program(vec![Instruction::Jmp {
        target: String::from("loop"),
        condition: Some(Condition {
            lhs: Operand::Register(String::from("rcx")),
            op: CompareOp::UnsignedLess,
            rhs: Operand::Register(String::from("rbx")),
        }),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  cmp rcx, rbx\n  jb loop\n"));
}

#[test]
fn labels_fall_through_without_implicit_jump() {
    let program = Program {
        entry: String::from("main"),
        memory: Vec::new(),
        labels: vec![
            Label {
                name: String::from("main"),
                instructions: vec![Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Register(String::from("rax"))),
                    value: AssignmentValue::Operand(Operand::Immediate(1)),
                }],
            },
            Label {
                name: String::from("next"),
                instructions: vec![Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Register(String::from("rbx"))),
                    value: AssignmentValue::Operand(Operand::Immediate(2)),
                }],
            },
        ],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("main:\n  mov rax, 1\n\nnext:\n  mov rbx, 2\n"));
}

#[test]
fn rejects_non_64_bit_push_register() {
    let program = main_program(vec![Instruction::Push {
        src: Operand::Register(String::from("eax")),
    }]);

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
                first: AddressTerm::Register(String::from("rsp")),
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
            name: String::from("value"),
            width: MemoryWidth::U64,
            value: Operand::Immediate(1),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Register(String::from("rax"))),
            value: AssignmentValue::Operand(Operand::Immediate(60)),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Register(String::from("rdi"))),
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
            name: String::from("value"),
            width: MemoryWidth::U64,
            value: Operand::Immediate(1),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Register(String::from("rax"))),
            value: AssignmentValue::Operand(Operand::Immediate(60)),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Register(String::from("rbx"))),
            value: AssignmentValue::Operand(Operand::Immediate(123)),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Register(String::from("rdi"))),
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
        parts: vec![PrintPart::Operand(Operand::Register(String::from("ah")))],
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
        dst: AssignmentTarget::Operand(Operand::Register(String::from("r8b"))),
        value: AssignmentValue::Operand(Operand::Register(String::from("ah"))),
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

#[test]
fn emits_float_memory_scalars() {
    let program = Program {
        entry: String::from("main"),
        memory: vec![
            MemoryDeclaration::FloatScalar {
                name: String::from("single"),
                width: MemoryWidth::F32,
                value: String::from("1.5"),
            },
            MemoryDeclaration::FloatScalar {
                name: String::from("double"),
                width: MemoryWidth::F64,
                value: String::from("-2.25"),
            },
        ],
        labels: vec![Label {
            name: String::from("main"),
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
        entry: String::from("main"),
        memory: vec![
            MemoryDeclaration::FloatScalar {
                name: String::from("single"),
                width: MemoryWidth::F32,
                value: String::from("1.5"),
            },
            MemoryDeclaration::FloatScalar {
                name: String::from("double"),
                width: MemoryWidth::F64,
                value: String::from("2.25"),
            },
        ],
        labels: vec![Label {
            name: String::from("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Register(String::from("xmm0"))),
                    value: AssignmentValue::Operand(Operand::Dereference {
                        address: Address {
                            first: AddressTerm::Ident(String::from("single")),
                            rest: Vec::new(),
                        },
                        width: Some(MemoryWidth::F32),
                    }),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Register(String::from("xmm1"))),
                    value: AssignmentValue::Operand(Operand::Dereference {
                        address: Address {
                            first: AddressTerm::Ident(String::from("double")),
                            rest: Vec::new(),
                        },
                        width: Some(MemoryWidth::F64),
                    }),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Dereference {
                        address: Address {
                            first: AddressTerm::Ident(String::from("single")),
                            rest: Vec::new(),
                        },
                        width: Some(MemoryWidth::F32),
                    }),
                    value: AssignmentValue::Operand(Operand::Register(String::from("xmm0"))),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Dereference {
                        address: Address {
                            first: AddressTerm::Ident(String::from("double")),
                            rest: Vec::new(),
                        },
                        width: Some(MemoryWidth::F64),
                    }),
                    value: AssignmentValue::Operand(Operand::Register(String::from("xmm1"))),
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
fn rejects_integer_register_float_memory_load() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(Operand::Register(String::from("rax"))),
        value: AssignmentValue::Operand(Operand::Dereference {
            address: Address {
                first: AddressTerm::Ident(String::from("double")),
                rest: Vec::new(),
            },
            width: Some(MemoryWidth::F64),
        }),
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
        dst: AssignmentTarget::Operand(Operand::Register(String::from("xmm0"))),
        value: AssignmentValue::Operand(Operand::Dereference {
            address: Address {
                first: AddressTerm::Register(String::from("rax")),
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
        dst: AssignmentTarget::Operand(Operand::Register(String::from("xmm0"))),
        value: AssignmentValue::Operand(Operand::Register(String::from("xmm1"))),
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
            dst: AssignmentTarget::Operand(Operand::Register(String::from("xmm0"))),
            value: AssignmentValue::FloatBinary {
                width: MemoryWidth::F32,
                op: FloatMathOp::Add,
                lhs: Operand::Register(String::from("xmm0")),
                rhs: Operand::Register(String::from("xmm1")),
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Register(String::from("xmm2"))),
            value: AssignmentValue::FloatBinary {
                width: MemoryWidth::F64,
                op: FloatMathOp::Multiply,
                lhs: Operand::Register(String::from("xmm3")),
                rhs: Operand::Register(String::from("xmm4")),
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
        entry: String::from("main"),
        memory: vec![
            MemoryDeclaration::FloatScalar {
                name: String::from("single"),
                width: MemoryWidth::F32,
                value: String::from("1.5"),
            },
            MemoryDeclaration::FloatScalar {
                name: String::from("double"),
                width: MemoryWidth::F64,
                value: String::from("2.25"),
            },
        ],
        labels: vec![Label {
            name: String::from("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Register(String::from("xmm0"))),
                    value: AssignmentValue::FloatBinary {
                        width: MemoryWidth::F32,
                        op: FloatMathOp::Subtract,
                        lhs: Operand::Register(String::from("xmm0")),
                        rhs: Operand::Dereference {
                            address: Address {
                                first: AddressTerm::Ident(String::from("single")),
                                rest: Vec::new(),
                            },
                            width: Some(MemoryWidth::F32),
                        },
                    },
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Register(String::from("xmm1"))),
                    value: AssignmentValue::FloatBinary {
                        width: MemoryWidth::F64,
                        op: FloatMathOp::Divide,
                        lhs: Operand::Dereference {
                            address: Address {
                                first: AddressTerm::Ident(String::from("double")),
                                rest: Vec::new(),
                            },
                            width: Some(MemoryWidth::F64),
                        },
                        rhs: Operand::Register(String::from("xmm2")),
                    },
                },
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
fn rejects_float_arithmetic_to_integer_register() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(Operand::Register(String::from("rax"))),
        value: AssignmentValue::FloatBinary {
            width: MemoryWidth::F64,
            op: FloatMathOp::Add,
            lhs: Operand::Register(String::from("xmm0")),
            rhs: Operand::Register(String::from("xmm1")),
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
        dst: AssignmentTarget::Operand(Operand::Register(String::from("xmm0"))),
        value: AssignmentValue::FloatBinary {
            width: MemoryWidth::F64,
            op: FloatMathOp::Add,
            lhs: Operand::Register(String::from("xmm0")),
            rhs: Operand::Dereference {
                address: Address {
                    first: AddressTerm::Register(String::from("rax")),
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
        entry: String::from("main"),
        memory: vec![MemoryDeclaration::Buffer {
            name: String::from("buf"),
            width: MemoryWidth::U8,
            count: 16,
        }],
        labels: vec![Label {
            name: String::from("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Register(String::from("rax"))),
                    value: AssignmentValue::Operand(Operand::Pointer(String::from("buf"))),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Dereference {
                        address: Address {
                            first: AddressTerm::Register(String::from("rax")),
                            rest: Vec::new(),
                        },
                        width: Some(MemoryWidth::U64),
                    }),
                    value: AssignmentValue::Operand(Operand::Immediate(i32::MAX as i128)),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::RegisterPair {
                        high: String::from("rdx"),
                        low: String::from("rax"),
                    },
                    value: AssignmentValue::WideMultiply {
                        signed: false,
                        lhs: Operand::Register(String::from("rbx")),
                        rhs: Operand::Register(String::from("rcx")),
                    },
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert_assembles(&asm);
}
