use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use subsea::ast::{
    Address, AddressOperator, AddressTerm, AssignmentTarget, AssignmentValue, BindingValue,
    CompareOp, Condition, ConditionExpr, ControlTarget, DataDeclaration, DataItem, ExprOp,
    Expression, FloatMathOp, Instruction, IntrinsicOp, Label, MathOp, MemoryDeclaration,
    MemoryValue, MemoryWidth, Operand, PairBinaryOp, PrintFormat, PrintPart, Program, ReadSource,
    RegisterPair, StringInitializer, StringProperty, WidthConversion,
};
use subsea::codegen::{
    Target, emit_x86_64_asm, emit_x86_64_asm_with_entry_symbol, emit_x86_64_linux_asm,
};

fn s(value: &str) -> String {
    value.to_string()
}

fn cmp(condition: Condition) -> ConditionExpr {
    ConditionExpr::Compare(condition)
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

fn rpair(high: &str, low: &str) -> RegisterPair {
    RegisterPair {
        high: s(high),
        low: s(low),
    }
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
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
        parts: vec![PrintPart::FormattedOperand {
            format: PrintFormat::SignedDecimal(MemoryWidth::I64),
            operand: reg("rax"),
        }],
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, rax\n"));
    assert!(asm.contains(".L.__subsea.main.print_1_loop:\n"));
    assert!(asm.contains("  div rbx\n"));
    assert!(asm.contains("  syscall\n"));
}

#[test]
fn emits_runtime_unsigned_decimal_print() {
    let program = main_program(vec![Instruction::Print {
        parts: vec![PrintPart::FormattedOperand {
            format: PrintFormat::UnsignedDecimal(MemoryWidth::U64),
            operand: reg("rax"),
        }],
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rbx, 10\n"));
    assert!(!asm.contains("  jl .L.__subsea.main.print_1_negative\n"));
}

#[test]
fn emits_runtime_signed_narrow_print() {
    let program = main_program(vec![Instruction::Print {
        parts: vec![
            PrintPart::FormattedOperand {
                format: PrintFormat::SignedDecimal(MemoryWidth::I8),
                operand: reg("al"),
            },
            PrintPart::FormattedOperand {
                format: PrintFormat::SignedDecimal(MemoryWidth::I32),
                operand: reg("ecx"),
            },
        ],
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  movsx rax, al\n"));
    assert!(asm.contains("  movsxd rax, ecx\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_runtime_unsigned_narrow_print() {
    let program = main_program(vec![Instruction::Print {
        parts: vec![
            PrintPart::FormattedOperand {
                format: PrintFormat::UnsignedDecimal(MemoryWidth::U8),
                operand: reg("al"),
            },
            PrintPart::FormattedOperand {
                format: PrintFormat::UnsignedDecimal(MemoryWidth::U32),
                operand: reg("ecx"),
            },
        ],
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  movzx rax, al\n"));
    assert!(asm.contains("  mov eax, ecx\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_runtime_hex_print_with_prefix() {
    let program = main_program(vec![Instruction::Print {
        parts: vec![PrintPart::FormattedOperand {
            format: PrintFormat::Hex,
            operand: reg("rax"),
        }],
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rbx, 16\n"));
    assert!(asm.contains("  mov byte ptr [rsi], 120\n"));
    assert!(asm.contains("  mov byte ptr [rsi], 48\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_runtime_binary_print_with_prefix() {
    let program = main_program(vec![Instruction::Print {
        parts: vec![PrintPart::FormattedOperand {
            format: PrintFormat::Binary,
            operand: reg("rax"),
        }],
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rbx, 2\n"));
    assert!(asm.contains("  mov byte ptr [rsi], 98\n"));
    assert!(asm.contains("  mov byte ptr [rsi], 48\n"));
    assert_assembles(&asm);
}

#[test]
fn rejects_narrow_runtime_print_format_operand() {
    let program = main_program(vec![Instruction::Print {
        parts: vec![PrintPart::FormattedOperand {
            format: PrintFormat::SignedDecimal(MemoryWidth::I64),
            operand: reg("eax"),
        }],
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "i64 print operand must be 64-bit, found 32-bit operand"
    );
}

#[test]
fn infers_stack_integer_print_formats() {
    let program = main_program(vec![
        Instruction::Stack {
            name: s("signed"),
            width: MemoryWidth::I64,
            value: Operand::Immediate(-1),
        },
        Instruction::Stack {
            name: s("unsigned"),
            width: MemoryWidth::U64,
            value: Operand::Immediate(1),
        },
        Instruction::Print {
            parts: vec![
                PrintPart::Binding(s("signed")),
                PrintPart::Binding(s("unsigned")),
            ],
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  jl .L.__subsea.main.print_1_negative\n"));
    assert!(!asm.contains("  jl .L.__subsea.main.print_2_negative\n"));
}

#[test]
fn infers_pointer_memory_print_format() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: vec![MemoryDeclaration::Scalar {
            name: s("address"),
            width: MemoryWidth::Ptr,
            value: 42,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Print {
                    parts: vec![PrintPart::FormattedOperand {
                        format: PrintFormat::Infer,
                        operand: deref_ident("address", None),
                    }],
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rbx, 16\n"));
    assert!(asm.contains("  mov byte ptr [rsi], 120\n"));
    assert_assembles(&asm);
}

#[test]
fn rejects_inferred_register_print_format() {
    let program = main_program(vec![Instruction::Print {
        parts: vec![PrintPart::FormattedOperand {
            format: PrintFormat::Infer,
            operand: reg("rax"),
        }],
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Cannot infer print format for register rax; use {i64}, {u64}, {x}, {b}, or {ptr}"
    );
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

    assert!(asm.contains("_start:\n  push rbp\n  mov rbp, rsp\n  sub rsp, 16\n"));
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
    assert!(asm.contains("_start:\n  push rbp\n  mov rbp, rsp\n  sub rsp, 16\n"));
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
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
fn emits_const_string_property_loads() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("message"),
            value: BindingValue::String(s("hello")),
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

    assert!(asm.contains("  mov rax, offset .Lstr_main_message\n"));
    assert!(asm.contains("  mov rbx, 5\n"));
    assert_assembles(&asm);
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
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
fn emits_linux_reserve() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::LinuxReserve {
            len: Operand::Immediate(4096),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rsi, 4096\n"));
    assert!(asm.contains("  mov rax, 9\n"));
    assert!(asm.contains("  mov rdi, 0\n"));
    assert!(asm.contains("  mov rdx, 3\n"));
    assert!(asm.contains("  mov r10, 34\n"));
    assert!(asm.contains("  mov r8, -1\n"));
    assert!(asm.contains("  mov r9, 0\n"));
    assert!(asm.contains("  syscall\n"));
}

#[test]
fn emits_linux_release() {
    let program = main_program(vec![Instruction::Release {
        ptr: reg("rbx"),
        len: Operand::Immediate(4096),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rdi, rbx\n"));
    assert!(asm.contains("  mov rsi, 4096\n"));
    assert!(asm.contains("  mov rax, 11\n"));
    assert!(asm.contains("  syscall\n"));
}

#[test]
fn emits_string_bytes_assignment_to_raw_memory() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(Operand::Dereference {
            address: Address {
                first: AddressTerm::Register(s("rax")),
                rest: Vec::new(),
            },
            width: None,
        }),
        value: AssignmentValue::StringBytes { value: s("Hi\n") },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov byte ptr [rax], 72\n"));
    assert!(asm.contains("  mov byte ptr [rax + 1], 105\n"));
    assert!(asm.contains("  mov byte ptr [rax + 2], 10\n"));
}

#[test]
fn emits_string_bytes_assignment_to_declared_memory_index() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: vec![MemoryDeclaration::Buffer {
            name: s("buf"),
            width: MemoryWidth::U8,
            count: 16,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Dereference {
                        address: Address {
                            first: AddressTerm::Ident(s("buf")),
                            rest: vec![(AddressOperator::Add, AddressTerm::Immediate(0))],
                        },
                        width: None,
                    }),
                    value: AssignmentValue::StringBytes { value: s("Hi\n") },
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov byte ptr [buf + 0], 72\n"));
    assert!(asm.contains("  mov byte ptr [buf + 0 + 1], 105\n"));
    assert!(asm.contains("  mov byte ptr [buf + 0 + 2], 10\n"));
}

#[test]
fn emits_string_binding_assignment_to_memory() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("msg"),
            value: BindingValue::String(s("Hi\n")),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Dereference {
                address: Address {
                    first: AddressTerm::Register(s("rax")),
                    rest: Vec::new(),
                },
                width: None,
            }),
            value: AssignmentValue::Operand(ident("msg")),
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov byte ptr [rax], 72\n"));
    assert!(asm.contains("  mov byte ptr [rax + 1], 105\n"));
    assert!(asm.contains("  mov byte ptr [rax + 2], 10\n"));
}

#[test]
fn rejects_empty_string_binding_assignment_to_memory() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("msg"),
            value: BindingValue::String(s("")),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(Operand::Dereference {
                address: Address {
                    first: AddressTerm::Register(s("rax")),
                    rest: Vec::new(),
                },
                width: None,
            }),
            value: AssignmentValue::Operand(ident("msg")),
        },
    ]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(error, "String byte assignment cannot be empty");
}

#[test]
fn rejects_string_bytes_assignment_to_register() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::StringBytes { value: s("Hi\n") },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "String byte assignment destination must be a memory operand"
    );
}

#[test]
fn rejects_string_bytes_assignment_with_explicit_width() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(Operand::Dereference {
            address: Address {
                first: AddressTerm::Register(s("rax")),
                rest: Vec::new(),
            },
            width: Some(MemoryWidth::U8),
        }),
        value: AssignmentValue::StringBytes { value: s("Hi\n") },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "String byte assignment destination cannot specify a memory width"
    );
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
            target: ControlTarget::Label(s("other")),
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
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![
            Label {
                name: s("main"),
                instructions: vec![Instruction::Jmp {
                    target: ControlTarget::Label(s("other")),
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
            target: ControlTarget::Label(s(".L.main.helper")),
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
fn emits_bitwise_and_or_xor() {
    let program = main_program(vec![
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Binary {
                op: MathOp::BitAnd,
                lhs: reg("rbx"),
                rhs: reg("rcx"),
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Binary {
                op: MathOp::BitOr,
                lhs: reg("rax"),
                rhs: Operand::Immediate(4),
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Binary {
                op: MathOp::BitXor,
                lhs: reg("rax"),
                rhs: reg("rdx"),
            },
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, rbx\n"));
    assert!(asm.contains("  and rax, rcx\n"));
    assert!(asm.contains("  or rax, 4\n"));
    assert!(asm.contains("  xor rax, rdx\n"));
}

#[test]
fn emits_bitwise_not() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::BitwiseUnary {
            op: subsea::ast::BitwiseUnaryOp::Not,
            operand: reg("rbx"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, rbx\n"));
    assert!(asm.contains("  not rax\n"));
}

#[test]
fn emits_bitwise_shifts() {
    let program = main_program(vec![
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Binary {
                op: MathOp::ShiftLeft,
                lhs: reg("rbx"),
                rhs: Operand::Immediate(3),
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rdx")),
            value: AssignmentValue::Binary {
                op: MathOp::ShiftRightLogical,
                lhs: reg("rdx"),
                rhs: reg("cl"),
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("r8")),
            value: AssignmentValue::Binary {
                op: MathOp::ShiftRightArithmetic,
                lhs: reg("r8"),
                rhs: Operand::Immediate(1),
            },
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, rbx\n"));
    assert!(asm.contains("  shl rax, 3\n"));
    assert!(asm.contains("  shr rdx, cl\n"));
    assert!(asm.contains("  sar r8, 1\n"));
}

#[test]
fn rejects_shift_count_other_than_immediate_or_cl() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Binary {
            op: MathOp::ShiftLeft,
            lhs: reg("rax"),
            rhs: reg("rcx"),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "shl count must be an immediate value or cl, found register rcx"
    );
}

#[test]
fn emits_zero_and_sign_extension() {
    let program = main_program(vec![
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Operand(Operand::Converted {
                operand: Box::new(reg("al")),
                conversion: WidthConversion::ZeroExtend,
            }),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rbx")),
            value: AssignmentValue::Operand(Operand::Converted {
                operand: Box::new(reg("cl")),
                conversion: WidthConversion::SignExtend,
            }),
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  movzx rax, al\n"));
    assert!(asm.contains("  movsx rbx, cl\n"));
}

#[test]
fn emits_32_to_64_bit_extensions() {
    let program = main_program(vec![
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::Operand(Operand::Converted {
                operand: Box::new(reg("ebx")),
                conversion: WidthConversion::ZeroExtend,
            }),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rdi")),
            value: AssignmentValue::Operand(Operand::Converted {
                operand: Box::new(reg("esi")),
                conversion: WidthConversion::SignExtend,
            }),
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov eax, ebx\n"));
    assert!(asm.contains("  movsxd rdi, esi\n"));
}

#[test]
fn emits_implicit_register_truncation() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: vec![MemoryDeclaration::Buffer {
            name: s("buf"),
            width: MemoryWidth::U8,
            count: 1,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("al")),
                    value: AssignmentValue::Operand(reg("rbx")),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(deref_ident("buf", Some(MemoryWidth::U8))),
                    value: AssignmentValue::Operand(reg("rax")),
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov al, bl\n"));
    assert!(asm.contains("  mov byte ptr [buf], al\n"));
}

#[test]
fn rejects_implicit_widening() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Operand(reg("al")),
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(error, "Cannot use 8-bit source with 64-bit destination");
}

#[test]
fn rejects_width_conversion_to_memory() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: vec![MemoryDeclaration::Buffer {
            name: s("buf"),
            width: MemoryWidth::U64,
            count: 1,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![Instruction::Assign {
                dst: AssignmentTarget::Operand(deref_ident("buf", Some(MemoryWidth::U64))),
                value: AssignmentValue::Operand(Operand::Converted {
                    operand: Box::new(reg("al")),
                    conversion: WidthConversion::ZeroExtend,
                }),
            }],
        }],
    };

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Width conversion destination must be an integer register"
    );
}

#[test]
fn emits_indexed_memory_load_and_store() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: vec![MemoryDeclaration::Buffer {
            name: s("values"),
            width: MemoryWidth::U64,
            count: 4,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("rax")),
                    value: AssignmentValue::Operand(Operand::Dereference {
                        address: subsea::ast::Address {
                            first: subsea::ast::AddressTerm::Ident(s("values")),
                            rest: vec![(
                                subsea::ast::AddressOperator::Add,
                                subsea::ast::AddressTerm::ScaledRegister {
                                    register: s("r8"),
                                    scale: 8,
                                },
                            )],
                        },
                        width: None,
                    }),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(Operand::Dereference {
                        address: subsea::ast::Address {
                            first: subsea::ast::AddressTerm::Ident(s("values")),
                            rest: vec![(
                                subsea::ast::AddressOperator::Add,
                                subsea::ast::AddressTerm::ScaledRegister {
                                    register: s("r8"),
                                    scale: 8,
                                },
                            )],
                        },
                        width: None,
                    }),
                    value: AssignmentValue::Operand(reg("rax")),
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, qword ptr [values + r8 * 8]\n"));
    assert!(asm.contains("  mov qword ptr [values + r8 * 8], rax\n"));
}

#[test]
fn emits_address_of_indexed_memory() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: vec![MemoryDeclaration::Buffer {
            name: s("buf"),
            width: MemoryWidth::U8,
            count: 16,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("rsi")),
                    value: AssignmentValue::Operand(Operand::AddressOf(subsea::ast::Address {
                        first: subsea::ast::AddressTerm::Ident(s("buf")),
                        rest: vec![(
                            subsea::ast::AddressOperator::Add,
                            subsea::ast::AddressTerm::Register(s("rax")),
                        )],
                    })),
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  lea rsi, [buf + rax]\n"));
}

#[test]
fn emits_address_of_raw_address_expression() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Operand(Operand::AddressOf(subsea::ast::Address {
            first: subsea::ast::AddressTerm::Register(s("rbx")),
            rest: vec![
                (
                    subsea::ast::AddressOperator::Add,
                    subsea::ast::AddressTerm::ScaledRegister {
                        register: s("rcx"),
                        scale: 4,
                    },
                ),
                (
                    subsea::ast::AddressOperator::Add,
                    subsea::ast::AddressTerm::Immediate(8),
                ),
            ],
        })),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  lea rax, [rbx + rcx * 4 + 8]\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_boolean_comparison_assignment() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Condition(cmp(Condition {
            lhs: reg("rdi"),
            op: CompareOp::SignedLess,
            rhs: reg("rsi"),
        })),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  cmp rdi, rsi\n"));
    assert!(asm.contains("  setl r10b\n"));
    assert!(asm.contains("  movzx rax, r10b\n"));
}

#[test]
fn emits_boolean_bitwise_and_zero_assignment() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("al")),
        value: AssignmentValue::Condition(ConditionExpr::BitwiseAndZero {
            lhs: reg("rax"),
            rhs: Operand::Immediate(8),
            op: CompareOp::NotEqual,
        }),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  test rax, 8\n"));
    assert!(asm.contains("  setne al\n"));
}

#[test]
fn emits_conditional_assignment_with_jump_around() {
    let program = main_program(vec![Instruction::AssignIf {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Operand(reg("rbx")),
        condition: cmp(Condition {
            lhs: reg("rcx"),
            op: CompareOp::Equal,
            rhs: Operand::Immediate(0),
        }),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  test rcx, rcx\n"));
    assert!(asm.contains("  jne .L.__subsea.main.assign_if_1_skip\n"));
    assert!(asm.contains("  mov rax, rbx\n"));
    assert!(asm.contains(".L.__subsea.main.assign_if_1_skip:\n"));
}

#[test]
fn emits_bitwise_and_zero_jump_as_test() {
    let program = main_program(vec![
        Instruction::Jmp {
            target: ControlTarget::Label(s(".L.main.set")),
            condition: Some(ConditionExpr::BitwiseAndZero {
                lhs: reg("rax"),
                rhs: Operand::Immediate(8),
                op: CompareOp::NotEqual,
            }),
        },
        Instruction::Label {
            name: s(".L.main.set"),
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  test rax, 8\n"));
    assert!(asm.contains("  jne .L.main.set\n"));
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
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
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
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
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
fn emits_pair_add_with_carry() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
        value: AssignmentValue::PairBinary {
            op: PairBinaryOp::Add,
            lhs: rpair("rdx", "rax"),
            rhs: rpair("rcx", "rbx"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  add rax, rbx\n"));
    assert!(asm.contains("  adc rdx, rcx\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_pair_subtract_with_borrow() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
        value: AssignmentValue::PairBinary {
            op: PairBinaryOp::Subtract,
            lhs: rpair("rdx", "rax"),
            rhs: rpair("rcx", "rbx"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  sub rax, rbx\n"));
    assert!(asm.contains("  sbb rdx, rcx\n"));
    assert_assembles(&asm);
}

#[test]
fn rejects_pair_arithmetic_when_left_pair_differs_from_destination() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
        value: AssignmentValue::PairBinary {
            op: PairBinaryOp::Add,
            lhs: rpair("r8", "r9"),
            rhs: rpair("rcx", "rbx"),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Pair arithmetic left operand must match destination; found rdx:rax = r8:r9 ..."
    );
}

#[test]
fn rejects_narrow_pair_arithmetic_register() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
        value: AssignmentValue::PairBinary {
            op: PairBinaryOp::Add,
            lhs: rpair("rdx", "rax"),
            rhs: rpair("ecx", "rbx"),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Pair arithmetic right high register must be 64-bit, found 32-bit register ecx"
    );
}

#[test]
fn rejects_pair_arithmetic_rhs_high_overlapping_destination_low() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
        value: AssignmentValue::PairBinary {
            op: PairBinaryOp::Add,
            lhs: rpair("rdx", "rax"),
            rhs: rpair("rax", "rbx"),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Pair arithmetic right high register rax cannot overlap destination low register rax"
    );
}

#[test]
fn rejects_pair_arithmetic_same_destination_register_family() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(rpair("rax", "eax")),
        value: AssignmentValue::PairBinary {
            op: PairBinaryOp::Add,
            lhs: rpair("rax", "eax"),
            rhs: rpair("rcx", "rbx"),
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Pair arithmetic destination registers must be different, found rax:eax"
    );
}

#[test]
fn rejects_pair_arithmetic_rbp_use_in_stack_label() {
    let program = main_program(vec![
        Instruction::Stack {
            name: s("value"),
            width: MemoryWidth::U64,
            value: Operand::Immediate(0),
        },
        Instruction::Assign {
            dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
            value: AssignmentValue::PairBinary {
                op: PairBinaryOp::Add,
                lhs: rpair("rdx", "rax"),
                rhs: rpair("rbp", "rbx"),
            },
        },
    ]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Label \"main\" declares stack variables, so rbp is reserved"
    );
}

#[test]
fn emits_unsigned_modulo_with_immediate_rhs() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rbx")),
        value: AssignmentValue::Expression(Expression::Binary {
            op: ExprOp::Modulo { signed: false },
            lhs: Box::new(Expression::Operand(reg("rbx"))),
            rhs: Box::new(Expression::Operand(Operand::Immediate(10))),
        }),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov r10, 10\n"));
    assert!(asm.contains("  mov rax, rbx\n"));
    assert!(asm.contains("  xor rdx, rdx\n"));
    assert!(asm.contains("  div r10\n"));
    assert!(asm.contains("  mov rbx, rdx\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_signed_divide_with_immediate_rhs() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rbx")),
        value: AssignmentValue::Expression(Expression::Binary {
            op: ExprOp::Divide { signed: true },
            lhs: Box::new(Expression::Operand(reg("rbx"))),
            rhs: Box::new(Expression::Operand(Operand::Immediate(10))),
        }),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  cqo\n"));
    assert!(asm.contains("  idiv r10\n"));
    assert!(asm.contains("  mov rbx, rax\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_power_with_immediate_exponent() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Binary {
            op: MathOp::Power,
            lhs: reg("rbx"),
            rhs: Operand::Immediate(3),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov r10, rbx\n"));
    assert!(asm.contains("  mov r11, 3\n"));
    assert!(asm.contains("  mov rax, 1\n"));
    assert!(asm.contains("  test r11, 1\n"));
    assert!(asm.contains("  imul rax, r10\n"));
    assert!(asm.contains("  imul r10, r10\n"));
    assert!(asm.contains("  shr r11, 1\n"));
    assert_assembles(&asm);
}

#[test]
fn does_not_copy_power_base_into_destination_first() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Expression(Expression::Binary {
            op: ExprOp::Power,
            lhs: Box::new(Expression::Operand(reg("rdx"))),
            rhs: Box::new(Expression::Operand(Operand::Immediate(3))),
        }),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov r10, rdx\n"));
    assert!(!asm.contains("  mov rax, rdx\n"));
    assert!(asm.contains("  test r11, r11\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_power_with_runtime_register_exponent() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Binary {
            op: MathOp::Power,
            lhs: reg("rbx"),
            rhs: reg("rcx"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov r10, rbx\n"));
    assert!(asm.contains("  mov r11, rcx\n"));
    assert!(asm.contains("  mov rax, 1\n"));
    assert!(asm.contains("  imul rax, r10\n"));
    assert_assembles(&asm);
}

#[test]
fn preserves_power_exponent_when_it_uses_r10() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Binary {
            op: MathOp::Power,
            lhs: reg("rbx"),
            rhs: reg("r10"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();
    let exponent_load = asm.find("  mov r11, r10\n").unwrap();
    let base_load = asm.find("  mov r10, rbx\n").unwrap();

    assert!(exponent_load < base_load);
    assert_assembles(&asm);
}

#[test]
fn zero_extends_narrow_power_register_exponent() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Binary {
            op: MathOp::Power,
            lhs: reg("rbx"),
            rhs: reg("cl"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  movzx r11, cl\n"));
    assert_assembles(&asm);
}

#[test]
fn zero_extends_narrow_power_memory_exponent() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: vec![MemoryDeclaration::Scalar {
            name: s("exp"),
            width: MemoryWidth::U8,
            value: 3,
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("rax")),
                    value: AssignmentValue::Binary {
                        op: MathOp::Power,
                        lhs: reg("rbx"),
                        rhs: deref_ident("exp", None),
                    },
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  movzx r11, byte ptr [exp]\n"));
    assert_assembles(&asm);
}

#[test]
fn preserves_expression_rhs_when_it_uses_destination_register() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::Expression(Expression::Binary {
            op: ExprOp::Math(MathOp::Add),
            lhs: Box::new(Expression::Operand(reg("rbx"))),
            rhs: Box::new(Expression::Binary {
                op: ExprOp::Math(MathOp::Multiply),
                lhs: Box::new(Expression::Operand(reg("rax"))),
                rhs: Box::new(Expression::Operand(Operand::Immediate(2))),
            }),
        }),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();
    let rhs_index = asm.find("  mov r10, rax\n").unwrap();
    let lhs_index = asm.find("  mov rax, rbx\n").unwrap();

    assert!(rhs_index < lhs_index);
    assert!(asm.contains("  imul r10, 2\n"));
    assert!(asm.contains("  add rax, r10\n"));
    assert_assembles(&asm);
}

#[test]
fn rejects_non_rdx_rax_widened_multiply_destination() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(rpair("r9", "r8")),
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
        dst: AssignmentTarget::RegisterPair(rpair("r9", "r8")),
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
fn emits_immediate_widened_multiply_rhs() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
        value: AssignmentValue::WideMultiply {
            signed: false,
            lhs: reg("rbx"),
            rhs: Operand::Immediate(2),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov r10, 2\n"));
    assert!(asm.contains("  mov rax, rbx\n"));
    assert!(asm.contains("  mul r10\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_immediate_widened_multiply_lhs() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
        value: AssignmentValue::WideMultiply {
            signed: false,
            lhs: Operand::Immediate(10),
            rhs: reg("rcx"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, 10\n"));
    assert!(asm.contains("  mul rcx\n"));
    assert_assembles(&asm);
}

#[test]
fn materializes_widened_multiply_rhs_that_uses_rax() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
        value: AssignmentValue::WideMultiply {
            signed: false,
            lhs: reg("rbx"),
            rhs: reg("rax"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov r10, rax\n"));
    assert!(asm.contains("  mov rax, rbx\n"));
    assert!(asm.contains("  mul r10\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_unsigned_widened_divide() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
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
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
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
fn emits_immediate_widened_divide_rhs() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
        value: AssignmentValue::WideDivide {
            signed: false,
            lhs: reg("rbx"),
            rhs: Operand::Immediate(2),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov r10, 2\n"));
    assert!(asm.contains("  xor rdx, rdx\n"));
    assert!(asm.contains("  div r10\n"));
    assert_assembles(&asm);
}

#[test]
fn materializes_widened_divide_rhs_that_uses_rdx() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
        value: AssignmentValue::WideDivide {
            signed: false,
            lhs: reg("rbx"),
            rhs: reg("rdx"),
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov r10, rdx\n"));
    assert!(asm.contains("  xor rdx, rdx\n"));
    assert!(asm.contains("  div r10\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_call_and_ret() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![
            Label {
                name: s("main"),
                instructions: vec![
                    Instruction::Call {
                        target: ControlTarget::Label(s("helper")),
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
fn emits_indirect_call_register() {
    let program = main_program(vec![Instruction::Call {
        target: ControlTarget::Operand(reg("rax")),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  call rax\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_indirect_jump_memory_operand() {
    let program = main_program(vec![Instruction::Jmp {
        target: ControlTarget::Operand(Operand::Dereference {
            address: addr_ident("handler"),
            width: Some(MemoryWidth::Ptr),
        }),
        condition: None,
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  jmp qword ptr [handler]\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_conditional_indirect_jump() {
    let program = main_program(vec![Instruction::Jmp {
        target: ControlTarget::Operand(reg("rax")),
        condition: Some(cmp(Condition {
            lhs: reg("rcx"),
            op: CompareOp::Equal,
            rhs: Operand::Immediate(0),
        })),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  test rcx, rcx\n"));
    assert!(asm.contains("  jne .L.__subsea.main.indirect_jmp_1_skip\n"));
    assert!(asm.contains("  jmp rax\n"));
    assert!(asm.contains(".L.__subsea.main.indirect_jmp_1_skip:\n"));
    assert_assembles(&asm);
}

#[test]
fn rejects_narrow_indirect_call_target() {
    let program = main_program(vec![Instruction::Call {
        target: ControlTarget::Operand(reg("eax")),
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "indirect call target must be 64-bit, found 32-bit operand"
    );
}

#[test]
fn emits_hlt() {
    let program = main_program(vec![Instruction::InlineAsm { text: s("hlt") }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  hlt\n"));
}

#[test]
fn emits_nop() {
    let program = main_program(vec![Instruction::Nop]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  nop\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_port_io() {
    let program = main_program(vec![
        Instruction::InlineAsm {
            text: s("out 0x80, al"),
        },
        Instruction::InlineAsm {
            text: s("in al, dx"),
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  out 0x80, al\n"));
    assert!(asm.contains("  in al, dx\n"));
    assert_assembles(&asm);
}

#[test]
fn freestanding_rejects_print() {
    let program = main_program(vec![Instruction::Print {
        parts: vec![PrintPart::Literal(s("hi"))],
    }]);

    let error = emit_x86_64_asm(&program, Target::X86_64Free).unwrap_err();

    assert_eq!(error, "print is only supported for target x86_64");
}

#[test]
fn freestanding_rejects_linux_reserve() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::LinuxReserve {
            len: Operand::Immediate(4096),
        },
    }]);

    let error = emit_x86_64_asm(&program, Target::X86_64Free).unwrap_err();

    assert_eq!(error, "reserve is only supported for target x86_64");
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
            target: ControlTarget::Label(s(".L.main.join")),
            condition: Some(cmp(Condition {
                lhs: reg("rax"),
                op: CompareOp::Equal,
                rhs: Operand::Immediate(0),
            })),
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
            target: ControlTarget::Label(s(".L.main.loop")),
            condition: None,
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("_start:\n.L.main.loop:\n  jmp .L.main.loop\n"));
}

#[test]
fn emits_program_entry_as_start_symbol() {
    let program = main_program(vec![]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains(".global _start\n\n_start:\n"));
    assert!(!asm.contains("\nmain:\n"));
    assert!(!asm.contains("jmp main"));
}

#[test]
fn rewrites_entry_label_references_to_start_symbol() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![
            Label {
                name: s("main"),
                instructions: vec![Instruction::Exit { code: 0 }],
            },
            Label {
                name: s("again"),
                instructions: vec![
                    Instruction::Call {
                        target: ControlTarget::Label(s("main")),
                    },
                    Instruction::Exit { code: 0 },
                ],
            },
        ],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("again:\n  call _start\n"));
}

#[test]
fn emits_custom_entry_symbol() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Label {
                    name: s(".L.main.hang"),
                },
                Instruction::InlineAsm { text: s("hlt") },
                Instruction::Jmp {
                    target: ControlTarget::Label(s(".L.main.hang")),
                    condition: None,
                },
            ],
        }],
    };

    let asm =
        emit_x86_64_asm_with_entry_symbol(&program, Target::X86_64Free, "kernel_entry").unwrap();

    assert!(asm.contains(".global kernel_entry\n\nkernel_entry:\n"));
    assert!(!asm.contains("\n_start:\n"));
}

#[test]
fn emits_custom_data_blocks() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: vec![DataDeclaration {
            name: s("request"),
            section: s(".requests"),
            align: Some(8),
            export: true,
            keep: true,
            items: vec![
                DataItem::Scalar {
                    width: MemoryWidth::U64,
                    value: 1,
                },
                DataItem::Addr {
                    target: s("response"),
                },
                DataItem::Zero { count: 16 },
                DataItem::Label {
                    name: s("response"),
                },
                DataItem::Scalar {
                    width: MemoryWidth::U64,
                    value: 0,
                },
            ],
        }],
        memory: Vec::new(),
        labels: vec![Label {
            name: s("main"),
            instructions: vec![Instruction::Exit { code: 0 }],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(
        asm.contains(
            ".section .requests, \"aR\", @progbits\n.global request\n.balign 8\nrequest:\n"
        )
    );
    assert!(asm.contains("  .quad 1\n"));
    assert!(asm.contains("  .quad response\n"));
    assert!(asm.contains("  .zero 16\n"));
    assert!(asm.contains("response:\n  .quad 0\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_signed_conditional_jump() {
    let program = main_program(vec![
        Instruction::Jmp {
            target: ControlTarget::Label(s(".L.main.done")),
            condition: Some(cmp(Condition {
                lhs: reg("rax"),
                op: CompareOp::SignedLess,
                rhs: Operand::Immediate(0),
            })),
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
            target: ControlTarget::Label(s(".L.main.loop")),
            condition: Some(cmp(Condition {
                lhs: reg("rcx"),
                op: CompareOp::UnsignedLess,
                rhs: reg("rbx"),
            })),
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  cmp rcx, rbx\n  jb .L.main.loop\n"));
}

#[test]
fn rejects_function_fallthrough() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
fn emits_initialized_memory_arrays_strings_repeats_and_addresses() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: vec![
            MemoryDeclaration::Array {
                name: s("values"),
                width: MemoryWidth::U16,
                values: vec![
                    MemoryValue::Integer(1),
                    MemoryValue::Integer(2),
                    MemoryValue::Integer(3),
                ],
            },
            MemoryDeclaration::Array {
                name: s("message"),
                width: MemoryWidth::U8,
                values: vec![MemoryValue::Integer(104), MemoryValue::Integer(105)],
            },
            MemoryDeclaration::Repeat {
                name: s("fill"),
                width: MemoryWidth::U8,
                count: 4,
                value: MemoryValue::Integer(255),
            },
            MemoryDeclaration::Array {
                name: s("callbacks"),
                width: MemoryWidth::Ptr,
                values: vec![
                    MemoryValue::Addr { target: s("main") },
                    MemoryValue::Addr {
                        target: s("handler"),
                    },
                ],
            },
        ],
        labels: vec![
            Label {
                name: s("main"),
                instructions: vec![Instruction::Exit { code: 0 }],
            },
            Label {
                name: s("handler"),
                instructions: vec![Instruction::Ret],
            },
        ],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("values:\n  .word 1\n  .word 2\n  .word 3\n"));
    assert!(asm.contains("message:\n  .byte 104\n  .byte 105\n"));
    assert!(asm.contains("fill:\n  .byte 255\n  .byte 255\n  .byte 255\n  .byte 255\n"));
    assert!(asm.contains("callbacks:\n  .quad _start\n  .quad handler\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_float_memory_scalars() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
fn emits_xmm_register_to_register_move() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("xmm0")),
        value: AssignmentValue::Operand(reg("xmm1")),
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  movaps xmm0, xmm1\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_integer_to_float_casts() {
    let program = main_program(vec![
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm0")),
            value: AssignmentValue::Operand(Operand::Cast {
                operand: Box::new(reg("rax")),
                width: MemoryWidth::F64,
            }),
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm1")),
            value: AssignmentValue::Operand(Operand::Cast {
                operand: Box::new(reg("ecx")),
                width: MemoryWidth::F32,
            }),
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  cvtsi2sd xmm0, rax\n"));
    assert!(asm.contains("  cvtsi2ss xmm1, ecx\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_float_memory_to_integer_casts() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: vec![MemoryDeclaration::FloatScalar {
            name: s("ratio"),
            width: MemoryWidth::F64,
            value: s("1.5"),
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("rax")),
                    value: AssignmentValue::Operand(Operand::Cast {
                        operand: Box::new(deref_ident("ratio", None)),
                        width: MemoryWidth::I64,
                    }),
                },
                Instruction::Assign {
                    dst: AssignmentTarget::Operand(reg("cx")),
                    value: AssignmentValue::Operand(Operand::Cast {
                        operand: Box::new(deref_ident("ratio", None)),
                        width: MemoryWidth::I16,
                    }),
                },
                Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  cvttsd2si rax, qword ptr [ratio]\n"));
    assert!(asm.contains("  cvttsd2si r11d, qword ptr [ratio]\n"));
    assert!(asm.contains("  mov cx, r11w\n"));
    assert_assembles(&asm);
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
fn emits_float_typed_intrinsic_calls() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("limit"),
            value: BindingValue::Float {
                value: s("2.0"),
                width: MemoryWidth::F64,
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm0")),
            value: AssignmentValue::IntrinsicCall {
                op: IntrinsicOp::Sqrt,
                width: MemoryWidth::F64,
                args: vec![float("4.0")],
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm0")),
            value: AssignmentValue::IntrinsicCall {
                op: IntrinsicOp::Min,
                width: MemoryWidth::F64,
                args: vec![reg("xmm0"), ident("limit")],
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm1")),
            value: AssignmentValue::IntrinsicCall {
                op: IntrinsicOp::Max,
                width: MemoryWidth::F32,
                args: vec![reg("xmm2"), reg("xmm3")],
            },
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains(".Lfloatval_main_limit:\n  .double 2.0\n"));
    assert!(asm.contains(".Lfloatlit_main_2:\n  .double 4.0\n"));
    assert!(asm.contains("  sqrtsd xmm0, qword ptr [rip + .Lfloatlit_main_2]\n"));
    assert!(asm.contains("  minsd xmm0, qword ptr [rip + .Lfloatval_main_limit]\n"));
    assert!(asm.contains("  movss xmm1, xmm2\n"));
    assert!(asm.contains("  maxss xmm1, xmm3\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_float_rounding_typed_intrinsic_calls() {
    let program = main_program(vec![
        Instruction::Const {
            name: s("ratio"),
            value: BindingValue::Float {
                value: s("2.5"),
                width: MemoryWidth::F64,
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm0")),
            value: AssignmentValue::IntrinsicCall {
                op: IntrinsicOp::Round,
                width: MemoryWidth::F64,
                args: vec![ident("ratio")],
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm1")),
            value: AssignmentValue::IntrinsicCall {
                op: IntrinsicOp::Floor,
                width: MemoryWidth::F32,
                args: vec![float("1.75")],
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm2")),
            value: AssignmentValue::IntrinsicCall {
                op: IntrinsicOp::Ceil,
                width: MemoryWidth::F64,
                args: vec![reg("xmm3")],
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("xmm4")),
            value: AssignmentValue::IntrinsicCall {
                op: IntrinsicOp::Trunc,
                width: MemoryWidth::F32,
                args: vec![reg("xmm5")],
            },
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains(".Lfloatval_main_ratio:\n  .double 2.5\n"));
    assert!(asm.contains(".Lfloatlit_main_2:\n  .float 1.75\n"));
    assert!(asm.contains("  roundsd xmm0, qword ptr [rip + .Lfloatval_main_ratio], 0\n"));
    assert!(asm.contains("  roundss xmm1, dword ptr [rip + .Lfloatlit_main_2], 1\n"));
    assert!(asm.contains("  roundsd xmm2, xmm3, 2\n"));
    assert!(asm.contains("  roundss xmm4, xmm5, 3\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_integer_typed_intrinsic_calls() {
    let program = main_program(vec![
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::IntrinsicCall {
                op: IntrinsicOp::Max,
                width: MemoryWidth::I64,
                args: vec![reg("rbx"), Operand::Immediate(5)],
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("al")),
            value: AssignmentValue::IntrinsicCall {
                op: IntrinsicOp::Min,
                width: MemoryWidth::U8,
                args: vec![reg("bl"), reg("cl")],
            },
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov rax, rbx\n"));
    assert!(asm.contains("  cmp rax, 5\n"));
    assert!(asm.contains("  jge .L.__subsea.main.max_"));
    assert!(asm.contains("  mov rax, 5\n"));
    assert!(asm.contains("  mov al, bl\n"));
    assert!(asm.contains("  cmp al, cl\n"));
    assert!(asm.contains("  jbe .L.__subsea.main.min_"));
    assert!(asm.contains("  mov al, cl\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_unsigned_integer_sqrt_typed_intrinsic_calls() {
    let program = main_program(vec![
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("rax")),
            value: AssignmentValue::IntrinsicCall {
                op: IntrinsicOp::Sqrt,
                width: MemoryWidth::U64,
                args: vec![reg("rbx")],
            },
        },
        Instruction::Assign {
            dst: AssignmentTarget::Operand(reg("eax")),
            value: AssignmentValue::IntrinsicCall {
                op: IntrinsicOp::Sqrt,
                width: MemoryWidth::U32,
                args: vec![Operand::Immediate(81)],
            },
        },
    ]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  mov r10, rbx\n"));
    assert!(asm.contains("  mov r11, 4611686018427387904\n"));
    assert!(asm.contains("  lea r8, [rax + r11]\n"));
    assert!(asm.contains("  mov r10d, 81\n"));
    assert!(asm.contains("  mov r11, 1073741824\n"));
    assert_assembles(&asm);
}

#[test]
fn emits_signed_integer_sqrt_typed_intrinsic_call() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::IntrinsicCall {
            op: IntrinsicOp::Sqrt,
            width: MemoryWidth::I64,
            args: vec![reg("rbx")],
        },
    }]);

    let asm = emit_x86_64_linux_asm(&program).unwrap();

    assert!(asm.contains("  bt r10, 63\n"));
    assert!(asm.contains("  jc .L.__subsea.main.sqrt_"));
    assert!(asm.contains("  ud2\n"));
    assert_assembles(&asm);
}

#[test]
fn rejects_negative_signed_integer_sqrt_immediate() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::IntrinsicCall {
            op: IntrinsicOp::Sqrt,
            width: MemoryWidth::I64,
            args: vec![Operand::Immediate(-1)],
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "Integer sqrt intrinsic signed operand must be non-negative"
    );
}

#[test]
fn rejects_integer_rounding_typed_intrinsic_call() {
    let program = main_program(vec![Instruction::Assign {
        dst: AssignmentTarget::Operand(reg("rax")),
        value: AssignmentValue::IntrinsicCall {
            op: IntrinsicOp::Round,
            width: MemoryWidth::I64,
            args: vec![reg("rbx")],
        },
    }]);

    let error = emit_x86_64_linux_asm(&program).unwrap_err();

    assert_eq!(
        error,
        "round only supports f32 or f64; integer rounding is not implemented"
    );
}

#[test]
fn emits_xmm_float_memory_arithmetic() {
    let program = Program {
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
            target: ControlTarget::Label(s(".L.main.done")),
            condition: Some(cmp(Condition {
                lhs: reg("xmm0"),
                op: CompareOp::FloatLess(MemoryWidth::F64),
                rhs: float("1.5"),
            })),
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
            target: ControlTarget::Label(s(".L.main.done")),
            condition: Some(cmp(Condition {
                lhs: reg("xmm0"),
                op: CompareOp::Less,
                rhs: ident("limit"),
            })),
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
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
        memory: vec![MemoryDeclaration::FloatScalar {
            name: s("limit"),
            width: MemoryWidth::F32,
            value: s("1.5"),
        }],
        labels: vec![Label {
            name: s("main"),
            instructions: vec![
                Instruction::Jmp {
                    target: ControlTarget::Label(s(".L.main.done")),
                    condition: Some(cmp(Condition {
                        lhs: reg("xmm0"),
                        op: CompareOp::LessEqual,
                        rhs: deref_ident("limit", None),
                    })),
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
            target: ControlTarget::Label(s(".L.main.done")),
            condition: Some(cmp(Condition {
                lhs: reg("rax"),
                op: CompareOp::Less,
                rhs: reg("rbx"),
            })),
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
        imports: Vec::new(),
        exports: Vec::new(),
        entry: s("main"),
        data: Vec::new(),
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
                    dst: AssignmentTarget::RegisterPair(rpair("rdx", "rax")),
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
