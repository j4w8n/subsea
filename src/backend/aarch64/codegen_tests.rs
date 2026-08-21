use crate::backend::{Architecture, Environment, RuntimeOperation, Target};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn aarch64_linux_target_describes_the_initial_cross_backend_contract() {
    let target = Target::AArch64Linux;
    let spec = target.spec();

    assert_eq!(target.name(), "aarch");
    assert_eq!(spec.architecture, Architecture::AArch64);
    assert_eq!(spec.environment, Environment::Linux);
    assert_eq!(spec.pointer_width, 64);
    assert_eq!(spec.pointer_alignment, 8);
    assert_eq!(spec.integer_argument_registers[0], "x0");
    assert_eq!(spec.integer_return_register, "x0");
    assert_eq!(spec.float_return_register, "v0");
    assert_eq!(spec.runtime_call_convention, "aapcs64");
    assert_eq!(spec.exit_syscall, Some((93, "x8", "x0")));
    assert!(target.supports_runtime(RuntimeOperation::Read));
    assert!(target.supports_runtime(RuntimeOperation::Reserve));
    assert!(target.supports_runtime(RuntimeOperation::Release));
    assert!(Target::parse("aarch").is_ok());
}

#[test]
fn aarch64_freestanding_target_shares_architecture_without_linux_runtime() {
    let target = Target::AArch64Free;
    let spec = target.spec();

    assert_eq!(target.name(), "aarch-free");
    assert_eq!(spec.architecture, Architecture::AArch64);
    assert_eq!(spec.environment, Environment::Freestanding);
    assert_eq!(spec.linker_emulation, "aarch64elf");
    assert_eq!(spec.stack_pointer, "sp");
    assert_eq!(spec.frame_pointer, "x29");
    assert!(!target.supports_runtime(RuntimeOperation::Exit));
    assert!(!target.supports_runtime(RuntimeOperation::Write));
    assert!(!target.supports_runtime(RuntimeOperation::Reserve));
    assert!(target.is_freestanding());
}

#[test]
fn aarch64_freestanding_codegen_rejects_linux_runtime_operations() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Exit { code: 0 }],
        }],
    };

    let error =
        super::emit_for_target_with_entry(&program, Target::AArch64Free, "_start").unwrap_err();

    assert_eq!(
        error.message,
        "AArch64 backend does not support linux.exit on freestanding target yet"
    );
}

#[test]
fn aarch64_freestanding_codegen_supports_custom_entry_symbols() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Nop],
        }],
    };

    let asm =
        super::emit_for_target_with_entry(&program, Target::AArch64Free, "kernel_entry").unwrap();

    assert!(asm.contains(".global kernel_entry"));
    assert!(asm.contains("kernel_entry:\n"));
}

#[test]
fn aarch64_emits_core_semantic_ir() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: vec![crate::ir::DataDeclaration {
            name: "answer".to_owned(),
            section: "rodata".to_owned(),
            align: Some(8),
            export: false,
            keep: false,
            items: vec![crate::ir::DataItem::Scalar {
                width: crate::ast::MemoryWidth::U64,
                value: 42,
            }],
        }],
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::Immediate(41)),
                },
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                    value: crate::ir::Value::Binary {
                        op: crate::ast::MathOp::Add,
                        lhs: crate::ir::Operand::TargetRegister("x0".to_owned()),
                        rhs: crate::ir::Operand::Immediate(1),
                    },
                },
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("  mov x0, #41\n"));
    assert!(asm.contains("  add x0, x0, #1\n"));
    assert!(asm.contains("  mov x8, #93\n  svc #0\n"));
}

#[test]
fn aarch64_emits_bitwise_and_shift_operations() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                    value: crate::ir::Value::Binary {
                        op: crate::ast::MathOp::BitAnd,
                        lhs: crate::ir::Operand::TargetRegister("x1".to_owned()),
                        rhs: crate::ir::Operand::TargetRegister("x2".to_owned()),
                    },
                },
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x3".to_owned()),
                    value: crate::ir::Value::Binary {
                        op: crate::ast::MathOp::ShiftLeft,
                        lhs: crate::ir::Operand::TargetRegister("x0".to_owned()),
                        rhs: crate::ir::Operand::Immediate(3),
                    },
                },
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x4".to_owned()),
                    value: crate::ir::Value::BitwiseUnary {
                        op: crate::ast::BitwiseUnaryOp::Not,
                        operand: crate::ir::Operand::TargetRegister("x3".to_owned()),
                    },
                },
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("  and x0, x1, x2\n"));
    assert!(asm.contains("  lsl x3, x0, #3\n"));
    assert!(asm.contains("  mvn x4, x3\n"));
}

#[test]
fn aarch64_emits_division_modulo_and_power_expressions() {
    let register = |name: &str| crate::ir::Operand::TargetRegister(name.to_owned());
    let operand = |name: &str| crate::ir::Value::Operand(register(name));
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: register("x0"),
                    value: crate::ir::Value::Expression {
                        op: crate::ast::ExprOp::Divide { signed: true },
                        lhs: Box::new(operand("x1")),
                        rhs: Box::new(operand("x2")),
                    },
                },
                crate::ir::Instruction::Assign {
                    dst: register("x3"),
                    value: crate::ir::Value::Expression {
                        op: crate::ast::ExprOp::Modulo { signed: false },
                        lhs: Box::new(operand("x4")),
                        rhs: Box::new(operand("x5")),
                    },
                },
                crate::ir::Instruction::Assign {
                    dst: register("x6"),
                    value: crate::ir::Value::Expression {
                        op: crate::ast::ExprOp::Power,
                        lhs: Box::new(operand("x7")),
                        rhs: Box::new(operand("x8")),
                    },
                },
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("sdiv x16, x16, x17"));
    assert!(asm.contains("udiv x18, x16, x17"));
    assert!(asm.contains("msub x16, x18, x17, x16"));
    assert!(asm.contains("mul x18, x18, x16"));
    assert!(asm.contains("cbz x17, .L.__subsea.aarch64.invalid_division_"));
    assert!(asm.contains("brk #0\n"));
}

#[test]
fn aarch64_emits_wide_multiply_and_divide() {
    let pair = crate::ir::RegisterPair {
        high: "x1".to_owned(),
        low: "x0".to_owned(),
    };
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::WideAssign {
                    dst: pair.clone(),
                    signed: false,
                    division: false,
                    lhs: crate::ir::Operand::TargetRegister("x2".to_owned()),
                    rhs: crate::ir::Operand::TargetRegister("x3".to_owned()),
                },
                crate::ir::Instruction::WideAssign {
                    dst: pair,
                    signed: true,
                    division: true,
                    lhs: crate::ir::Operand::TargetRegister("x4".to_owned()),
                    rhs: crate::ir::Operand::TargetRegister("x5".to_owned()),
                },
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("mul x0, x16, x17"));
    assert!(asm.contains("umulh x1, x16, x17"));
    assert!(asm.contains("sdiv x0, x16, x17"));
    assert!(asm.contains("msub x1, x0, x17, x16"));
}

#[test]
fn aarch64_emits_linux_syscall_instruction() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Syscall,
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("  svc #0\n"));
}

#[test]
fn aarch64_emits_string_bytes_assignment() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::Memory {
                        address: crate::ir::Address {
                            first: crate::ir::AddressTerm::TargetRegister("x0".to_owned()),
                            rest: Vec::new(),
                        },
                        width: Some(crate::ast::MemoryWidth::U8),
                    },
                    value: crate::ir::Value::StringBytes {
                        value: "Hi".to_owned(),
                    },
                },
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("mov w17, #72"));
    assert!(asm.contains("strb w17, [x16]"));
    assert!(asm.contains("mov w17, #105"));
    assert!(asm.contains("strb w17, [x16, #1]"));
}

#[test]
fn aarch64_uses_declared_integer_memory_width_for_loads_and_stores() {
    let address = || crate::ir::Address {
        first: crate::ir::AddressTerm::TargetRegister("x0".to_owned()),
        rest: Vec::new(),
    };
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::Memory {
                        address: address(),
                        width: Some(crate::ast::MemoryWidth::U8),
                    },
                    value: crate::ir::Value::Operand(crate::ir::Operand::TargetRegister(
                        "w1".to_owned(),
                    )),
                },
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x2".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::Memory {
                        address: address(),
                        width: Some(crate::ast::MemoryWidth::I8),
                    }),
                },
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("strb w1, [x0]"));
    assert!(asm.contains("ldrsb x2, [x0]"));
}

#[test]
fn aarch64_emits_integer_sqrt_and_inferred_printing() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                    value: crate::ir::Value::IntrinsicCall {
                        op: crate::ast::IntrinsicOp::Sqrt,
                        width: crate::ast::MemoryWidth::U64,
                        args: vec![crate::ir::Operand::TargetRegister("x1".to_owned())],
                    },
                },
                crate::ir::Instruction::Runtime(crate::ir::RuntimeOperation::Print {
                    parts: vec![crate::ir::PrintPart::FormattedOperand {
                        format: crate::ir::PrintFormat::Infer,
                        operand: crate::ir::Operand::Memory {
                            address: crate::ir::Address {
                                first: crate::ir::AddressTerm::TargetRegister("x2".to_owned()),
                                rest: Vec::new(),
                            },
                            width: Some(crate::ast::MemoryWidth::U64),
                        },
                    }],
                }),
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("sqrt_loop_"));
    assert!(asm.contains("ldr x16, [x2]"));
    assert!(asm.contains("mov x18, #10"));
}

#[test]
fn aarch64_uses_narrow_integer_intrinsic_registers() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("w0".to_owned()),
                    value: crate::ir::Value::IntrinsicCall {
                        op: crate::ast::IntrinsicOp::Min,
                        width: crate::ast::MemoryWidth::U8,
                        args: vec![
                            crate::ir::Operand::TargetRegister("w1".to_owned()),
                            crate::ir::Operand::TargetRegister("w2".to_owned()),
                        ],
                    },
                },
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("cmp w16, w17"));
    assert!(asm.contains("csel w0, w16, w17, lo"));
}

#[test]
fn aarch64_stores_float_operations_and_intrinsics_to_memory() {
    let memory = |name: &str| crate::ir::Operand::Memory {
        address: crate::ir::Address {
            first: crate::ir::AddressTerm::Name(name.to_owned()),
            rest: Vec::new(),
        },
        width: Some(crate::ast::MemoryWidth::F32),
    };
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: memory("result"),
                    value: crate::ir::Value::FloatBinary {
                        width: crate::ast::MemoryWidth::F32,
                        op: crate::ast::FloatMathOp::Add,
                        lhs: crate::ir::Operand::TargetRegister("v1".to_owned()),
                        rhs: crate::ir::Operand::TargetRegister("v2".to_owned()),
                    },
                },
                crate::ir::Instruction::Assign {
                    dst: memory("result"),
                    value: crate::ir::Value::IntrinsicCall {
                        op: crate::ast::IntrinsicOp::Sqrt,
                        width: crate::ast::MemoryWidth::F32,
                        args: vec![crate::ir::Operand::TargetRegister("v3".to_owned())],
                    },
                },
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("fadd s16, s16, s17"));
    assert!(asm.contains("str s16, [result]"));
    assert!(asm.contains("fsqrt s16, s16"));
}

#[test]
fn aarch64_formats_scalar_stack_bindings_by_declared_width() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout {
                slots: vec![crate::ir::StackSlot::Scalar {
                    name: "count".to_owned(),
                    width: crate::ast::MemoryWidth::U32,
                }],
            },
            instructions: vec![
                crate::ir::Instruction::Runtime(crate::ir::RuntimeOperation::Print {
                    parts: vec![crate::ir::PrintPart::Binding("count".to_owned())],
                }),
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("ldr x16, [x29, #48]"));
    assert!(asm.contains("mov x18, #10"));
}

#[test]
fn aarch64_formats_compile_time_print_bindings() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Const {
                    name: "message".to_owned(),
                    value: crate::ir::ConstValue::String("hello".to_owned()),
                },
                crate::ir::Instruction::Const {
                    name: "count".to_owned(),
                    value: crate::ir::ConstValue::Integer {
                        value: 7,
                        width: Some(crate::ast::MemoryWidth::U32),
                    },
                },
                crate::ir::Instruction::Const {
                    name: "ratio".to_owned(),
                    value: crate::ir::ConstValue::Float {
                        value: "1.5".to_owned(),
                        width: crate::ast::MemoryWidth::F64,
                    },
                },
                crate::ir::Instruction::Runtime(crate::ir::RuntimeOperation::Print {
                    parts: vec![
                        crate::ir::PrintPart::Binding("message".to_owned()),
                        crate::ir::PrintPart::Binding("count".to_owned()),
                        crate::ir::PrintPart::Binding("ratio".to_owned()),
                    ],
                }),
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains(".byte 104, 101, 108, 108, 111"));
    assert!(asm.contains("mov x16, #7"));
    assert!(asm.contains("mov x18, #10"));
    assert!(asm.contains(".byte 49, 46, 53"));
}

#[test]
fn aarch64_emits_floating_point_operations() {
    let register = |name: &str| crate::ir::Operand::TargetRegister(name.to_owned());
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: register("v0"),
                    value: crate::ir::Value::FloatBinary {
                        width: crate::ast::MemoryWidth::F32,
                        op: crate::ast::FloatMathOp::Add,
                        lhs: register("v1"),
                        rhs: register("v2"),
                    },
                },
                crate::ir::Instruction::Assign {
                    dst: register("v3"),
                    value: crate::ir::Value::FloatBinary {
                        width: crate::ast::MemoryWidth::F64,
                        op: crate::ast::FloatMathOp::Multiply,
                        lhs: crate::ir::Operand::FloatLiteral("1.5".to_owned()),
                        rhs: register("v4"),
                    },
                },
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("fadd s0, s16, s17"));
    assert!(asm.contains("fmul d3, d16, d17"));
    assert!(asm.contains(".double 1.5"));
}

#[test]
fn aarch64_emits_loads_and_stores_that_assemble() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::Immediate(7)),
                },
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::Memory {
                        address: crate::ir::Address {
                            first: crate::ir::AddressTerm::TargetRegister("x1".to_owned()),
                            rest: Vec::new(),
                        },
                        width: Some(crate::ast::MemoryWidth::U64),
                    },
                    value: crate::ir::Value::Operand(crate::ir::Operand::TargetRegister(
                        "x0".to_owned(),
                    )),
                },
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::Memory {
                        address: crate::ir::Address {
                            first: crate::ir::AddressTerm::TargetRegister("x1".to_owned()),
                            rest: Vec::new(),
                        },
                        width: Some(crate::ast::MemoryWidth::U64),
                    }),
                },
                crate::ir::Instruction::AssignIf {
                    dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::Immediate(1)),
                    condition: crate::ir::Condition::Compare {
                        lhs: crate::ir::Operand::TargetRegister("s0".to_owned()),
                        op: crate::ast::CompareOp::FloatEqual(crate::ast::MemoryWidth::F32),
                        rhs: crate::ir::Operand::TargetRegister("s1".to_owned()),
                    },
                },
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };
    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();
    let Some(mut child) = Command::new("aarch64-linux-gnu-as")
        .arg("-o")
        .arg("/tmp/subsea-aarch64-backend.o")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .ok()
    else {
        return;
    };
    child
        .stdin
        .take()
        .unwrap()
        .write_all(asm.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    let _ = std::fs::remove_file("/tmp/subsea-aarch64-backend.o");
}

#[test]
fn aarch64_lowers_scalar_stack_slots() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout {
                slots: vec![crate::ir::StackSlot::Scalar {
                    name: "local".to_owned(),
                    width: crate::ast::MemoryWidth::U64,
                }],
            },
            instructions: vec![
                crate::ir::Instruction::Stack {
                    name: "local".to_owned(),
                    width: crate::ast::MemoryWidth::U64,
                    value: crate::ir::Operand::Immediate(9),
                },
                crate::ir::Instruction::Ret,
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("  sub sp, sp, #64\n"));
    assert!(asm.contains("  stp x29, x30, [sp]\n  mov x29, sp\n"));
    assert!(asm.contains("  str x16, [x29, #48]\n"));
    assert!(asm.contains("  ldp x29, x30, [sp]\n  add sp, sp, #64\n  ret\n"));
}

#[test]
fn aarch64_emits_linux_write_runtime_operation() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Runtime(crate::ir::RuntimeOperation::Print {
                    parts: vec![crate::ir::PrintPart::Literal("hi\n".to_owned())],
                }),
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("mov x8, #64"));
    assert!(asm.contains("mov x2, #3"));
    assert!(asm.contains("svc #0"));
}

#[test]
fn aarch64_emits_linux_memory_runtime_operations() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x20".to_owned()),
                    value: crate::ir::Value::PlatformReserve {
                        len: crate::ir::Operand::Immediate(4096),
                    },
                },
                crate::ir::Instruction::Runtime(crate::ir::RuntimeOperation::Read {
                    source: crate::ir::ReadSource::Stdin,
                    dst: crate::ir::Operand::TargetRegister("x20".to_owned()),
                    len: crate::ir::Operand::Immediate(16),
                }),
                crate::ir::Instruction::Runtime(crate::ir::RuntimeOperation::Release {
                    ptr: crate::ir::Operand::TargetRegister("x20".to_owned()),
                    len: crate::ir::Operand::Immediate(4096),
                }),
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("mov x8, #222"));
    assert!(asm.contains("mov x8, #63"));
    assert!(asm.contains("mov x8, #215"));
}

#[test]
fn aarch64_resolves_stack_operands_in_linux_memory_operations() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout {
                slots: vec![
                    crate::ir::StackSlot::Scalar {
                        name: "ptr".to_owned(),
                        width: crate::ast::MemoryWidth::Ptr,
                    },
                    crate::ir::StackSlot::Scalar {
                        name: "len".to_owned(),
                        width: crate::ast::MemoryWidth::U64,
                    },
                ],
            },
            instructions: vec![
                crate::ir::Instruction::Runtime(crate::ir::RuntimeOperation::Read {
                    source: crate::ir::ReadSource::Stdin,
                    dst: crate::ir::Operand::Name("ptr".to_owned()),
                    len: crate::ir::Operand::Name("len".to_owned()),
                }),
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                    value: crate::ir::Value::PlatformReserve {
                        len: crate::ir::Operand::Name("len".to_owned()),
                    },
                },
                crate::ir::Instruction::Runtime(crate::ir::RuntimeOperation::Release {
                    ptr: crate::ir::Operand::Name("ptr".to_owned()),
                    len: crate::ir::Operand::Name("len".to_owned()),
                }),
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("mov x1, x29\n  add x1, x1, #48"));
    assert!(asm.contains("ldr x2, [x29, #56]"));
    assert!(asm.contains("ldr x1, [x29, #56]"));
    assert!(asm.contains("ldr x0, [x29, #48]"));
}

#[test]
fn aarch64_emits_stack_string_runtime_printing() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout {
                slots: vec![crate::ir::StackSlot::String {
                    name: "message".to_owned(),
                }],
            },
            instructions: vec![
                crate::ir::Instruction::StackString {
                    name: "message".to_owned(),
                    value: crate::ir::StringInitializer::Literal("hello\n".to_owned()),
                },
                crate::ir::Instruction::Runtime(crate::ir::RuntimeOperation::Print {
                    parts: vec![crate::ir::PrintPart::Binding("message".to_owned())],
                }),
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("ldr x1, [x29, #48]"));
    assert!(asm.contains("ldr x2, [x29, #56]"));
    assert!(asm.contains("mov x8, #64"));
}

#[test]
fn aarch64_infers_stack_string_properties_for_runtime_printing() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout {
                slots: vec![crate::ir::StackSlot::String {
                    name: "message".to_owned(),
                }],
            },
            instructions: vec![
                crate::ir::Instruction::StackString {
                    name: "message".to_owned(),
                    value: crate::ir::StringInitializer::Literal("hello".to_owned()),
                },
                crate::ir::Instruction::Runtime(crate::ir::RuntimeOperation::Print {
                    parts: vec![
                        crate::ir::PrintPart::FormattedOperand {
                            format: crate::ir::PrintFormat::Infer,
                            operand: crate::ir::Operand::StringProperty {
                                name: "message".to_owned(),
                                property: crate::ir::StringProperty::Len,
                            },
                        },
                        crate::ir::PrintPart::FormattedOperand {
                            format: crate::ir::PrintFormat::Infer,
                            operand: crate::ir::Operand::StringProperty {
                                name: "message".to_owned(),
                                property: crate::ir::StringProperty::Ptr,
                            },
                        },
                    ],
                }),
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("ldr x16, [x29, #56]"));
    assert!(asm.contains("ldr x16, [x29, #48]"));
    assert!(asm.contains("mov x8, #64"));
}

#[test]
fn aarch64_qemu_runtime_output_when_available() {
    if Command::new("qemu-aarch64")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }

    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Runtime(crate::ir::RuntimeOperation::Print {
                    parts: vec![crate::ir::PrintPart::Literal("qemu\n".to_owned())],
                }),
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };
    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();
    let base = format!("/tmp/subsea-aarch64-{}", std::process::id());
    let asm_path = format!("{base}.s");
    let obj_path = format!("{base}.o");
    let bin_path = format!("{base}.elf");
    std::fs::write(&asm_path, asm).unwrap();
    assert!(
        Command::new("aarch64-linux-gnu-as")
            .arg(&asm_path)
            .arg("-o")
            .arg(&obj_path)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("aarch64-linux-gnu-ld")
            .arg(&obj_path)
            .arg("-o")
            .arg(&bin_path)
            .status()
            .unwrap()
            .success()
    );
    let output = Command::new("qemu-aarch64")
        .arg(&bin_path)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"qemu\n");
    let _ = std::fs::remove_file(asm_path);
    let _ = std::fs::remove_file(obj_path);
    let _ = std::fs::remove_file(bin_path);
}

#[test]
fn aarch64_emits_address_of_indexed_memory() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::AddressOf(
                        crate::ir::Address {
                            first: crate::ir::AddressTerm::Name("buffer".to_owned()),
                            rest: vec![
                                (
                                    crate::ir::AddressOperator::Add,
                                    crate::ir::AddressTerm::ScaledTargetRegister {
                                        register: "x1".to_owned(),
                                        scale: 4,
                                    },
                                ),
                                (
                                    crate::ir::AddressOperator::Add,
                                    crate::ir::AddressTerm::Immediate(8),
                                ),
                                (
                                    crate::ir::AddressOperator::Subtract,
                                    crate::ir::AddressTerm::TargetRegister("x2".to_owned()),
                                ),
                            ],
                        },
                    )),
                },
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x3".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::Memory {
                        address: crate::ir::Address {
                            first: crate::ir::AddressTerm::TargetRegister("x3".to_owned()),
                            rest: vec![(
                                crate::ir::AddressOperator::Add,
                                crate::ir::AddressTerm::ScaledTargetRegister {
                                    register: "x4".to_owned(),
                                    scale: 8,
                                },
                            )],
                        },
                        width: Some(crate::ast::MemoryWidth::U64),
                    }),
                },
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("add x0, x0, x1, lsl #2\n"));
    assert!(asm.contains("add x0, x0, #8\n"));
    assert!(asm.contains("sub x0, x0, x2\n"));
    assert!(asm.contains("ldr x3, [x3, x4, lsl #3]\n"));

    let Some(mut child) = Command::new("aarch64-linux-gnu-as")
        .arg("-o")
        .arg("/tmp/subsea-aarch64-address.o")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .ok()
    else {
        return;
    };
    child
        .stdin
        .take()
        .unwrap()
        .write_all(asm.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    let _ = std::fs::remove_file("/tmp/subsea-aarch64-address.o");
}

#[test]
fn aarch64_rejects_symbol_address_scratch_conflicts() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Assign {
                dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                value: crate::ir::Value::Operand(crate::ir::Operand::AddressOf(
                    crate::ir::Address {
                        first: crate::ir::AddressTerm::TargetRegister("x14".to_owned()),
                        rest: vec![(
                            crate::ir::AddressOperator::Add,
                            crate::ir::AddressTerm::Name("buffer".to_owned()),
                        )],
                    },
                )),
            }],
        }],
    };

    let error =
        super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap_err();

    assert_eq!(
        error.message,
        "AArch64 backend does not support address uses the symbol scratch register yet"
    );
}

#[test]
fn aarch64_rejects_narrow_index_registers() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Assign {
                dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                value: crate::ir::Value::Operand(crate::ir::Operand::AddressOf(
                    crate::ir::Address {
                        first: crate::ir::AddressTerm::TargetRegister("w1".to_owned()),
                        rest: Vec::new(),
                    },
                )),
            }],
        }],
    };

    let error =
        super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap_err();

    assert_eq!(
        error.message,
        "AArch64 address register must be a 64-bit x register, found w1"
    );
}

#[test]
fn aarch64_rejects_invalid_register_and_memory_moves() {
    let register_program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Assign {
                dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                value: crate::ir::Value::Operand(crate::ir::Operand::TargetRegister(
                    "s0".to_owned(),
                )),
            }],
        }],
    };
    let error =
        super::emit_for_target_with_entry(&register_program, Target::AArch64Linux, "_start")
            .unwrap_err();
    assert_eq!(
        error.message,
        "AArch64 register move cannot mix integer and floating-point registers"
    );

    let memory_program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Assign {
                dst: crate::ir::Operand::Memory {
                    address: crate::ir::Address {
                        first: crate::ir::AddressTerm::Name("value".to_owned()),
                        rest: Vec::new(),
                    },
                    width: Some(crate::ast::MemoryWidth::U64),
                },
                value: crate::ir::Value::Operand(crate::ir::Operand::Memory {
                    address: crate::ir::Address {
                        first: crate::ir::AddressTerm::Name("ratio".to_owned()),
                        rest: Vec::new(),
                    },
                    width: Some(crate::ast::MemoryWidth::F64),
                }),
            }],
        }],
    };
    let error = super::emit_for_target_with_entry(&memory_program, Target::AArch64Linux, "_start")
        .unwrap_err();
    assert_eq!(
        error.message,
        "AArch64 integer memory destination cannot use a floating-point source"
    );
}

#[test]
fn aarch64_push_pop_assembly_is_valid() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Push {
                    src: crate::ir::Operand::TargetRegister("x0".to_owned()),
                },
                crate::ir::Instruction::Pop {
                    dst: crate::ir::Operand::TargetRegister("x1".to_owned()),
                },
            ],
        }],
    };
    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();
    let Some(mut child) = Command::new("aarch64-linux-gnu-as")
        .arg("-o")
        .arg("/tmp/subsea-aarch64-push-pop.o")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .ok()
    else {
        return;
    };
    child
        .stdin
        .take()
        .unwrap()
        .write_all(asm.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    let _ = std::fs::remove_file("/tmp/subsea-aarch64-push-pop.o");
}

#[test]
fn aarch64_supports_forward_string_constant_properties() {
    let property = |property| crate::ir::Operand::StringProperty {
        name: "message".to_owned(),
        property,
    };
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Runtime(crate::ir::RuntimeOperation::Print {
                    parts: vec![crate::ir::PrintPart::FormattedOperand {
                        format: crate::ir::PrintFormat::Infer,
                        operand: property(crate::ir::StringProperty::Len),
                    }],
                }),
                crate::ir::Instruction::Const {
                    name: "message".to_owned(),
                    value: crate::ir::ConstValue::String("hello".to_owned()),
                },
                crate::ir::Instruction::Runtime(crate::ir::RuntimeOperation::Print {
                    parts: vec![crate::ir::PrintPart::FormattedOperand {
                        format: crate::ir::PrintFormat::Infer,
                        operand: property(crate::ir::StringProperty::Ptr),
                    }],
                }),
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("mov x16, #5\n"));
    assert!(asm.contains(".L.__subsea.aarch64.const_string_"));
}

#[test]
fn aarch64_converts_forward_integer_constants() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::Converted {
                        operand: Box::new(crate::ir::Operand::Name("count".to_owned())),
                        conversion: crate::ir::WidthConversion::ZeroExtend,
                    }),
                },
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x1".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::Name("count".to_owned())),
                },
                crate::ir::Instruction::Const {
                    name: "count".to_owned(),
                    value: crate::ir::ConstValue::Integer {
                        value: 255,
                        width: Some(crate::ast::MemoryWidth::U8),
                    },
                },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("mov x16, #255\n  uxtb x0, w16\n"));
    assert!(asm.contains("mov x1, #255\n"));
}

#[test]
fn aarch64_rejects_float_binary_scratch_aliases() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Assign {
                dst: crate::ir::Operand::TargetRegister("v0".to_owned()),
                value: crate::ir::Value::FloatBinary {
                    width: crate::ast::MemoryWidth::F32,
                    op: crate::ast::FloatMathOp::Add,
                    lhs: crate::ir::Operand::Memory {
                        address: crate::ir::Address {
                            first: crate::ir::AddressTerm::Name("left".to_owned()),
                            rest: Vec::new(),
                        },
                        width: Some(crate::ast::MemoryWidth::F32),
                    },
                    rhs: crate::ir::Operand::TargetRegister("s16".to_owned()),
                },
            }],
        }],
    };

    let error =
        super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap_err();

    assert_eq!(
        error.message,
        "AArch64 floating-point arithmetic right operand conflicts with v16 scratch register"
    );
}

#[test]
fn aarch64_emits_float_memory_loads() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("s0".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::Memory {
                        address: crate::ir::Address {
                            first: crate::ir::AddressTerm::Name("ratio".to_owned()),
                            rest: Vec::new(),
                        },
                        width: Some(crate::ast::MemoryWidth::F32),
                    }),
                },
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::Memory {
                        address: crate::ir::Address {
                            first: crate::ir::AddressTerm::Name("out".to_owned()),
                            rest: Vec::new(),
                        },
                        width: Some(crate::ast::MemoryWidth::F32),
                    },
                    value: crate::ir::Value::Operand(crate::ir::Operand::TargetRegister(
                        "s0".to_owned(),
                    )),
                },
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("s1".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::Memory {
                        address: crate::ir::Address {
                            first: crate::ir::AddressTerm::Name("ratio".to_owned()),
                            rest: Vec::new(),
                        },
                        width: Some(crate::ast::MemoryWidth::F32),
                    }),
                },
                crate::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("adrp x15, ratio\n  add x15, x15, :lo12:ratio\n  ldr s0, [x15]\n"));
    assert!(asm.contains("str s16, [x15]\n"));
    assert!(asm.contains("ldr s1, [x15]\n"));

    let Some(mut child) = Command::new("aarch64-linux-gnu-as")
        .arg("-o")
        .arg("/tmp/subsea-aarch64-float-memory.o")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .ok()
    else {
        return;
    };
    child
        .stdin
        .take()
        .unwrap()
        .write_all(asm.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    let _ = std::fs::remove_file("/tmp/subsea-aarch64-float-memory.o");
}

#[test]
fn aarch64_rejects_float_memory_copy_width_mismatch() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Assign {
                dst: crate::ir::Operand::Memory {
                    address: crate::ir::Address {
                        first: crate::ir::AddressTerm::Name("out".to_owned()),
                        rest: Vec::new(),
                    },
                    width: Some(crate::ast::MemoryWidth::F32),
                },
                value: crate::ir::Value::Operand(crate::ir::Operand::Memory {
                    address: crate::ir::Address {
                        first: crate::ir::AddressTerm::Name("ratio".to_owned()),
                        rest: Vec::new(),
                    },
                    width: Some(crate::ast::MemoryWidth::F64),
                }),
            }],
        }],
    };

    let error =
        super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap_err();

    assert_eq!(
        error.message,
        "AArch64 floating-point operand width must be f32, found f64"
    );
}

#[test]
fn aarch64_preserves_static_data_metadata_and_entry_references() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: vec![crate::ir::DataDeclaration {
            name: "entry_ref".to_owned(),
            section: "rodata".to_owned(),
            align: None,
            export: false,
            keep: true,
            items: vec![crate::ir::DataItem::Address {
                target: "main".to_owned(),
            }],
        }],
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Nop],
        }],
    };

    let asm =
        super::emit_for_target_with_entry(&program, Target::AArch64Free, "kernel_entry").unwrap();

    assert!(asm.contains(".section .rodata, \"aR\", @progbits\n"));
    assert!(asm.contains("  .quad kernel_entry\n"));

    let Some(mut child) = Command::new("aarch64-linux-gnu-as")
        .arg("-o")
        .arg("/tmp/subsea-aarch64-static-data.o")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .ok()
    else {
        return;
    };
    child
        .stdin
        .take()
        .unwrap()
        .write_all(asm.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    let _ = std::fs::remove_file("/tmp/subsea-aarch64-static-data.o");
}

#[test]
fn aarch64_supports_memory_pop_and_float_comparisons() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Pop {
                    dst: crate::ir::Operand::Memory {
                        address: crate::ir::Address {
                            first: crate::ir::AddressTerm::Name("value".to_owned()),
                            rest: Vec::new(),
                        },
                        width: Some(crate::ast::MemoryWidth::U64),
                    },
                },
                crate::ir::Instruction::AssignIf {
                    dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::Immediate(1)),
                    condition: crate::ir::Condition::Compare {
                        lhs: crate::ir::Operand::TargetRegister("s0".to_owned()),
                        op: crate::ast::CompareOp::FloatLess(crate::ast::MemoryWidth::F32),
                        rhs: crate::ir::Operand::FloatLiteral("1.0".to_owned()),
                    },
                },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains(
        "ldr x16, [sp], #16\n  adrp x15, value\n  add x15, x15, :lo12:value\n  str x16, [x15]\n"
    ));
    assert!(asm.contains("fcmp s16, s17\n"));
    assert!(asm.contains("b.ge"));
}

#[test]
fn aarch64_emits_and_validates_indirect_control_flow() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Call {
                    target: crate::ir::ControlTarget::Operand(crate::ir::Operand::TargetRegister(
                        "x3".to_owned(),
                    )),
                },
                crate::ir::Instruction::Jmp {
                    target: crate::ir::ControlTarget::Operand(crate::ir::Operand::Memory {
                        address: crate::ir::Address {
                            first: crate::ir::AddressTerm::Name("handler".to_owned()),
                            rest: Vec::new(),
                        },
                        width: Some(crate::ast::MemoryWidth::Ptr),
                    }),
                    condition: None,
                },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("mov x16, x3\n  blr x16\n"));
    assert!(asm.contains("ldr x16, [x15]\n  br x16\n"));
    if let Some(mut child) = Command::new("aarch64-linux-gnu-as")
        .arg("-o")
        .arg("/tmp/subsea-aarch64-control.o")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .ok()
    {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(asm.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
        let _ = std::fs::remove_file("/tmp/subsea-aarch64-control.o");
    }
}

#[test]
fn aarch64_rejects_integer_scratch_register_conflicts() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Assign {
                dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                value: crate::ir::Value::Binary {
                    op: crate::ast::MathOp::Add,
                    lhs: crate::ir::Operand::Memory {
                        address: crate::ir::Address {
                            first: crate::ir::AddressTerm::Name("value".to_owned()),
                            rest: Vec::new(),
                        },
                        width: Some(crate::ast::MemoryWidth::U64),
                    },
                    rhs: crate::ir::Operand::TargetRegister("x16".to_owned()),
                },
            }],
        }],
    };

    let error =
        super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap_err();

    assert_eq!(
        error.message,
        "AArch64 integer arithmetic cannot use x16 as the right register when the left operand needs scratch x16"
    );
}

#[test]
fn aarch64_rejects_negative_power_exponents() {
    let operand =
        |name: &str| crate::ir::Value::Operand(crate::ir::Operand::TargetRegister(name.to_owned()));
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Assign {
                dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                value: crate::ir::Value::Expression {
                    op: crate::ast::ExprOp::Power,
                    lhs: Box::new(operand("x1")),
                    rhs: Box::new(crate::ir::Value::Operand(crate::ir::Operand::Immediate(-1))),
                },
            }],
        }],
    };

    let error =
        super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap_err();

    assert_eq!(error.message, "Power exponent must be non-negative");
}

#[test]
fn aarch64_preserves_left_value_across_nested_right_expression() {
    let register =
        |name: &str| crate::ir::Value::Operand(crate::ir::Operand::TargetRegister(name.to_owned()));
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Assign {
                dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                value: crate::ir::Value::Expression {
                    op: crate::ast::ExprOp::Divide { signed: true },
                    lhs: Box::new(register("x1")),
                    rhs: Box::new(crate::ir::Value::Expression {
                        op: crate::ast::ExprOp::Math(crate::ast::MathOp::Add),
                        lhs: Box::new(register("x2")),
                        rhs: Box::new(register("x3")),
                    }),
                },
            }],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("str x16, [sp, #-16]!\n"));
    assert!(asm.contains("ldr x16, [sp], #16\n"));
    assert!(asm.contains("sdiv x16, x16, x17\n"));
}

#[test]
fn aarch64_emits_pair_arithmetic_and_rejects_narrow_pairs() {
    let pair = |high: &str, low: &str| crate::ir::RegisterPair {
        high: high.to_owned(),
        low: low.to_owned(),
    };
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::PairAssign {
                dst: pair("x1", "x0"),
                op: crate::ast::PairBinaryOp::Add,
                lhs: pair("x1", "x0"),
                rhs: pair("x3", "x2"),
            }],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("adds x0, x0, x2\n  adc x1, x1, x3\n"));
    if let Some(mut child) = Command::new("aarch64-linux-gnu-as")
        .arg("-o")
        .arg("/tmp/subsea-aarch64-pair.o")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .ok()
    {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(asm.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
        let _ = std::fs::remove_file("/tmp/subsea-aarch64-pair.o");
    }

    let invalid = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::PairAssign {
                dst: pair("x1", "w0"),
                op: crate::ast::PairBinaryOp::Add,
                lhs: pair("x1", "w0"),
                rhs: pair("x3", "x2"),
            }],
        }],
    };

    let error =
        super::emit_for_target_with_entry(&invalid, Target::AArch64Linux, "_start").unwrap_err();

    assert_eq!(
        error.message,
        "Pair arithmetic destination low register must be 64-bit, found 32-bit register w0"
    );
}

#[test]
fn aarch64_emits_narrow_integer_width_conversions() {
    let converted = |width, conversion| {
        crate::ir::Value::Operand(crate::ir::Operand::Converted {
            operand: Box::new(crate::ir::Operand::Memory {
                address: crate::ir::Address {
                    first: crate::ir::AddressTerm::Name("value".to_owned()),
                    rest: Vec::new(),
                },
                width: Some(width),
            }),
            conversion,
        })
    };
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                    value: converted(
                        crate::ast::MemoryWidth::U8,
                        crate::ir::WidthConversion::ZeroExtend,
                    ),
                },
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x1".to_owned()),
                    value: converted(
                        crate::ast::MemoryWidth::I16,
                        crate::ir::WidthConversion::SignExtend,
                    ),
                },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("ldrb w16, [x15]\n  uxtb x0, w16\n"));
    assert!(asm.contains("ldrsh x16, [x15]\n  sxth x1, w16\n"));
}

#[test]
fn aarch64_emits_signed_and_unsigned_float_casts() {
    let memory = |name: &str, width: crate::ast::MemoryWidth| crate::ir::Operand::Memory {
        address: crate::ir::Address {
            first: crate::ir::AddressTerm::Name(name.to_owned()),
            rest: Vec::new(),
        },
        width: Some(width),
    };
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("v0".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::Cast {
                        operand: Box::new(memory("count", crate::ast::MemoryWidth::U64)),
                        width: crate::ast::MemoryWidth::F64,
                    }),
                },
                crate::ir::Instruction::Assign {
                    dst: crate::ir::Operand::TargetRegister("x1".to_owned()),
                    value: crate::ir::Value::Operand(crate::ir::Operand::Cast {
                        operand: Box::new(memory("ratio", crate::ast::MemoryWidth::F64)),
                        width: crate::ast::MemoryWidth::U64,
                    }),
                },
            ],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("ucvtf d0, x16\n"));
    assert!(asm.contains("fcvtzu x1, d16\n"));
    assert!(asm.contains("brk #0\n"));
}

#[test]
fn aarch64_uses_ordered_float_comparison_branches() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout {
                slots: vec![crate::ir::StackSlot::Scalar {
                    name: "left".to_owned(),
                    width: crate::ast::MemoryWidth::F64,
                }],
            },
            instructions: vec![crate::ir::Instruction::Jmp {
                target: crate::ir::ControlTarget::Label("done".to_owned()),
                condition: Some(crate::ir::Condition::Compare {
                    lhs: crate::ir::Operand::Name("left".to_owned()),
                    op: crate::ast::CompareOp::FloatEqual(crate::ast::MemoryWidth::F64),
                    rhs: crate::ir::Operand::FloatLiteral("0.0".to_owned()),
                }),
            }],
        }],
    };

    let asm = super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap();

    assert!(asm.contains("b.ne .L.__subsea.aarch64.jmp_skip_"));
    assert!(asm.contains("b.vs .L.__subsea.aarch64.jmp_skip_"));
}

#[test]
fn aarch64_rejects_mixed_float_register_widths() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::AssignIf {
                dst: crate::ir::Operand::TargetRegister("x0".to_owned()),
                value: crate::ir::Value::Operand(crate::ir::Operand::Immediate(1)),
                condition: crate::ir::Condition::Compare {
                    lhs: crate::ir::Operand::TargetRegister("s0".to_owned()),
                    op: crate::ast::CompareOp::FloatEqual(crate::ast::MemoryWidth::F32),
                    rhs: crate::ir::Operand::TargetRegister("d1".to_owned()),
                },
            }],
        }],
    };

    let error =
        super::emit_for_target_with_entry(&program, Target::AArch64Linux, "_start").unwrap_err();

    assert_eq!(
        error.message,
        "AArch64 backend does not support floating-point destination register yet"
    );
}
