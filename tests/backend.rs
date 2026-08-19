use std::io::Write;
use std::process::{Command, Stdio};
use subsea::backend::{
    Architecture, EntryConvention, Environment, FramePointerPolicy, RuntimeOperation, Target,
};

#[test]
fn x86_64_linux_target_describes_its_backend_properties() {
    let target = Target::X86_64;
    let spec = target.spec();

    assert_eq!(spec.architecture, Architecture::X86_64);
    assert_eq!(spec.environment, Environment::Linux);
    assert_eq!(spec.pointer_width, 64);
    assert_eq!(spec.pointer_alignment, 8);
    assert_eq!(spec.linker_emulation, "elf_x86_64");
    assert_eq!(spec.stack_alignment, 16);
    assert_eq!(spec.stack_pointer, "rsp");
    assert_eq!(spec.frame_pointer, "rbp");
    assert_eq!(spec.frame_pointer_policy, FramePointerPolicy::Required);
    assert_eq!(spec.entry_convention, EntryConvention::ProcessEntry);
    assert_eq!(spec.runtime_call_convention, "sysv_amd64");
    assert_eq!(
        spec.integer_argument_registers,
        ["rdi", "rsi", "rdx", "rcx", "r8", "r9"]
    );
    assert_eq!(spec.integer_return_register, "rax");
    assert_eq!(spec.float_return_register, "xmm0");
    assert!(target.supports_runtime(RuntimeOperation::Exit));
    assert!(target.supports_runtime(RuntimeOperation::Reserve));
    assert!(!target.is_freestanding());
}

#[test]
fn x86_64_freestanding_target_shares_architecture_but_changes_environment() {
    let target = Target::X86_64Free;
    let spec = target.spec();

    assert_eq!(spec.architecture, Architecture::X86_64);
    assert_eq!(spec.environment, Environment::Freestanding);
    assert_eq!(spec.pointer_width, 64);
    assert_eq!(spec.pointer_alignment, 8);
    assert_eq!(spec.linker_emulation, "elf_x86_64");
    assert_eq!(spec.stack_alignment, 16);
    assert_eq!(spec.stack_pointer, "rsp");
    assert_eq!(spec.frame_pointer, "rbp");
    assert_eq!(spec.frame_pointer_policy, FramePointerPolicy::Required);
    assert_eq!(spec.entry_convention, EntryConvention::ProcessEntry);
    assert_eq!(spec.runtime_call_convention, "sysv_amd64");
    assert_eq!(spec.integer_argument_registers[0], "rdi");
    assert!(!target.supports_runtime(RuntimeOperation::Exit));
    assert!(!target.supports_runtime(RuntimeOperation::Reserve));
    assert!(target.is_freestanding());
}

#[test]
fn target_names_and_parsing_remain_stable() {
    for (name, expected) in [
        ("x86", Target::X86_64),
        ("x86-free", Target::X86_64Free),
        ("aarch", Target::AArch64Linux),
        ("aarch-free", Target::AArch64Free),
    ] {
        assert_eq!(Target::parse(name), Ok(expected));
    }
    assert_eq!(Target::X86_64.name(), "x86");
    assert_eq!(Target::AArch64Linux.name(), "aarch");
}

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
    for register in ["x0", "x30", "w0", "sp", "wsp", "v0", "q31"] {
        assert!(subsea::backend::aarch64::is_register(register));
    }
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
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![subsea::ir::Instruction::Exit { code: 0 }],
        }],
    };

    let error =
        subsea::backend::aarch64::emit_for_target(&program, Target::AArch64Free).unwrap_err();

    assert_eq!(
        error.message,
        "AArch64 backend does not support linux.exit on freestanding target yet"
    );
}

#[test]
fn aarch64_freestanding_codegen_supports_custom_entry_symbols() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![subsea::ir::Instruction::Nop],
        }],
    };

    let asm = subsea::backend::aarch64::emit_for_target_with_entry(
        &program,
        Target::AArch64Free,
        "kernel_entry",
    )
    .unwrap();

    assert!(asm.contains(".global kernel_entry"));
    assert!(asm.contains("kernel_entry:\n"));
}

#[test]
fn aarch64_emits_core_semantic_ir() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: vec![subsea::ir::DataDeclaration {
            name: "answer".to_owned(),
            section: "rodata".to_owned(),
            align: Some(8),
            export: false,
            keep: false,
            items: vec![subsea::ir::DataItem::Scalar {
                width: subsea::ast::MemoryWidth::U64,
                value: 42,
            }],
        }],
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::TargetRegister("x0".to_owned()),
                    value: subsea::ir::Value::Operand(subsea::ir::Operand::Immediate(41)),
                },
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::TargetRegister("x0".to_owned()),
                    value: subsea::ir::Value::Binary {
                        op: subsea::ast::MathOp::Add,
                        lhs: subsea::ir::Operand::TargetRegister("x0".to_owned()),
                        rhs: subsea::ir::Operand::Immediate(1),
                    },
                },
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("  mov x0, #41\n"));
    assert!(asm.contains("  add x0, x0, #1\n"));
    assert!(asm.contains("  mov x8, #93\n  svc #0\n"));
}

#[test]
fn aarch64_emits_bitwise_and_shift_operations() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::TargetRegister("x0".to_owned()),
                    value: subsea::ir::Value::Binary {
                        op: subsea::ast::MathOp::BitAnd,
                        lhs: subsea::ir::Operand::TargetRegister("x1".to_owned()),
                        rhs: subsea::ir::Operand::TargetRegister("x2".to_owned()),
                    },
                },
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::TargetRegister("x3".to_owned()),
                    value: subsea::ir::Value::Binary {
                        op: subsea::ast::MathOp::ShiftLeft,
                        lhs: subsea::ir::Operand::TargetRegister("x0".to_owned()),
                        rhs: subsea::ir::Operand::Immediate(3),
                    },
                },
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::TargetRegister("x4".to_owned()),
                    value: subsea::ir::Value::BitwiseUnary {
                        op: subsea::ast::BitwiseUnaryOp::Not,
                        operand: subsea::ir::Operand::TargetRegister("x3".to_owned()),
                    },
                },
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("  and x0, x1, x2\n"));
    assert!(asm.contains("  lsl x3, x0, #3\n"));
    assert!(asm.contains("  mvn x4, x3\n"));
}

#[test]
fn aarch64_emits_division_modulo_and_power_expressions() {
    let register = |name: &str| subsea::ir::Operand::TargetRegister(name.to_owned());
    let operand = |name: &str| subsea::ir::Value::Operand(register(name));
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Assign {
                    dst: register("x0"),
                    value: subsea::ir::Value::Expression {
                        op: subsea::ast::ExprOp::Divide { signed: true },
                        lhs: Box::new(operand("x1")),
                        rhs: Box::new(operand("x2")),
                    },
                },
                subsea::ir::Instruction::Assign {
                    dst: register("x3"),
                    value: subsea::ir::Value::Expression {
                        op: subsea::ast::ExprOp::Modulo { signed: false },
                        lhs: Box::new(operand("x4")),
                        rhs: Box::new(operand("x5")),
                    },
                },
                subsea::ir::Instruction::Assign {
                    dst: register("x6"),
                    value: subsea::ir::Value::Expression {
                        op: subsea::ast::ExprOp::Power,
                        lhs: Box::new(operand("x7")),
                        rhs: Box::new(operand("x8")),
                    },
                },
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("sdiv x16, x16, x17"));
    assert!(asm.contains("udiv x18, x16, x17"));
    assert!(asm.contains("msub x16, x18, x17, x16"));
    assert!(asm.contains("mul x18, x18, x16"));
}

#[test]
fn aarch64_emits_wide_multiply_and_divide() {
    let pair = subsea::ir::RegisterPair {
        high: "x1".to_owned(),
        low: "x0".to_owned(),
    };
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::WideAssign {
                    dst: pair.clone(),
                    signed: false,
                    division: false,
                    lhs: subsea::ir::Operand::TargetRegister("x2".to_owned()),
                    rhs: subsea::ir::Operand::TargetRegister("x3".to_owned()),
                },
                subsea::ir::Instruction::WideAssign {
                    dst: pair,
                    signed: true,
                    division: true,
                    lhs: subsea::ir::Operand::TargetRegister("x4".to_owned()),
                    rhs: subsea::ir::Operand::TargetRegister("x5".to_owned()),
                },
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("mul x0, x16, x17"));
    assert!(asm.contains("umulh x1, x16, x17"));
    assert!(asm.contains("sdiv x0, x16, x17"));
    assert!(asm.contains("msub x1, x0, x17, x16"));
}

#[test]
fn aarch64_emits_linux_syscall_instruction() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Syscall,
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("  svc #0\n"));
}

#[test]
fn aarch64_emits_string_bytes_assignment() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::Memory {
                        address: subsea::ir::Address {
                            first: subsea::ir::AddressTerm::TargetRegister("x0".to_owned()),
                            rest: Vec::new(),
                        },
                        width: Some(subsea::ast::MemoryWidth::U8),
                    },
                    value: subsea::ir::Value::StringBytes {
                        value: "Hi".to_owned(),
                    },
                },
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("mov w17, #72"));
    assert!(asm.contains("strb w17, [x16]"));
    assert!(asm.contains("mov w17, #105"));
    assert!(asm.contains("strb w17, [x16, #1]"));
}

#[test]
fn aarch64_uses_declared_integer_memory_width_for_loads_and_stores() {
    let address = || subsea::ir::Address {
        first: subsea::ir::AddressTerm::TargetRegister("x0".to_owned()),
        rest: Vec::new(),
    };
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::Memory {
                        address: address(),
                        width: Some(subsea::ast::MemoryWidth::U8),
                    },
                    value: subsea::ir::Value::Operand(subsea::ir::Operand::TargetRegister(
                        "w1".to_owned(),
                    )),
                },
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::TargetRegister("x2".to_owned()),
                    value: subsea::ir::Value::Operand(subsea::ir::Operand::Memory {
                        address: address(),
                        width: Some(subsea::ast::MemoryWidth::I8),
                    }),
                },
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("strb w1, [x0]"));
    assert!(asm.contains("ldrsb x2, [x0]"));
}

#[test]
fn aarch64_emits_integer_sqrt_and_inferred_printing() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::TargetRegister("x0".to_owned()),
                    value: subsea::ir::Value::IntrinsicCall {
                        op: subsea::ast::IntrinsicOp::Sqrt,
                        width: subsea::ast::MemoryWidth::U64,
                        args: vec![subsea::ir::Operand::TargetRegister("x1".to_owned())],
                    },
                },
                subsea::ir::Instruction::Runtime(subsea::ir::RuntimeOperation::Print {
                    parts: vec![subsea::ir::PrintPart::FormattedOperand {
                        format: subsea::ir::PrintFormat::Infer,
                        operand: subsea::ir::Operand::Memory {
                            address: subsea::ir::Address {
                                first: subsea::ir::AddressTerm::TargetRegister("x2".to_owned()),
                                rest: Vec::new(),
                            },
                            width: Some(subsea::ast::MemoryWidth::U64),
                        },
                    }],
                }),
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("sqrt_loop_"));
    assert!(asm.contains("ldr x16, [x2]"));
    assert!(asm.contains("mov x18, #10"));
}

#[test]
fn aarch64_uses_narrow_integer_intrinsic_registers() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::TargetRegister("w0".to_owned()),
                    value: subsea::ir::Value::IntrinsicCall {
                        op: subsea::ast::IntrinsicOp::Min,
                        width: subsea::ast::MemoryWidth::U8,
                        args: vec![
                            subsea::ir::Operand::TargetRegister("w1".to_owned()),
                            subsea::ir::Operand::TargetRegister("w2".to_owned()),
                        ],
                    },
                },
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("cmp w16, w17"));
    assert!(asm.contains("csel w0, w16, w17, lo"));
}

#[test]
fn aarch64_stores_float_operations_and_intrinsics_to_memory() {
    let memory = |name: &str| subsea::ir::Operand::Memory {
        address: subsea::ir::Address {
            first: subsea::ir::AddressTerm::Name(name.to_owned()),
            rest: Vec::new(),
        },
        width: Some(subsea::ast::MemoryWidth::F32),
    };
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Assign {
                    dst: memory("result"),
                    value: subsea::ir::Value::FloatBinary {
                        width: subsea::ast::MemoryWidth::F32,
                        op: subsea::ast::FloatMathOp::Add,
                        lhs: subsea::ir::Operand::TargetRegister("v1".to_owned()),
                        rhs: subsea::ir::Operand::TargetRegister("v2".to_owned()),
                    },
                },
                subsea::ir::Instruction::Assign {
                    dst: memory("result"),
                    value: subsea::ir::Value::IntrinsicCall {
                        op: subsea::ast::IntrinsicOp::Sqrt,
                        width: subsea::ast::MemoryWidth::F32,
                        args: vec![subsea::ir::Operand::TargetRegister("v3".to_owned())],
                    },
                },
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("fadd s16, s16, s17"));
    assert!(asm.contains("str s16, [result]"));
    assert!(asm.contains("fsqrt s16, s16"));
}

#[test]
fn aarch64_formats_scalar_stack_bindings_by_declared_width() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout {
                slots: vec![subsea::ir::StackSlot::Scalar {
                    name: "count".to_owned(),
                    width: subsea::ast::MemoryWidth::U32,
                }],
            },
            instructions: vec![
                subsea::ir::Instruction::Runtime(subsea::ir::RuntimeOperation::Print {
                    parts: vec![subsea::ir::PrintPart::Binding("count".to_owned())],
                }),
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("ldr x16, [x29, #48]"));
    assert!(asm.contains("mov x18, #10"));
}

#[test]
fn aarch64_formats_compile_time_print_bindings() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Const {
                    name: "message".to_owned(),
                    value: subsea::ir::ConstValue::String("hello".to_owned()),
                },
                subsea::ir::Instruction::Const {
                    name: "count".to_owned(),
                    value: subsea::ir::ConstValue::Integer {
                        value: 7,
                        width: Some(subsea::ast::MemoryWidth::U32),
                    },
                },
                subsea::ir::Instruction::Const {
                    name: "ratio".to_owned(),
                    value: subsea::ir::ConstValue::Float {
                        value: "1.5".to_owned(),
                        width: subsea::ast::MemoryWidth::F64,
                    },
                },
                subsea::ir::Instruction::Runtime(subsea::ir::RuntimeOperation::Print {
                    parts: vec![
                        subsea::ir::PrintPart::Binding("message".to_owned()),
                        subsea::ir::PrintPart::Binding("count".to_owned()),
                        subsea::ir::PrintPart::Binding("ratio".to_owned()),
                    ],
                }),
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains(".byte 104, 101, 108, 108, 111"));
    assert!(asm.contains("mov x16, #7"));
    assert!(asm.contains("mov x18, #10"));
    assert!(asm.contains(".byte 49, 46, 53"));
}

#[test]
fn aarch64_emits_floating_point_operations() {
    let register = |name: &str| subsea::ir::Operand::TargetRegister(name.to_owned());
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Assign {
                    dst: register("v0"),
                    value: subsea::ir::Value::FloatBinary {
                        width: subsea::ast::MemoryWidth::F32,
                        op: subsea::ast::FloatMathOp::Add,
                        lhs: register("v1"),
                        rhs: register("v2"),
                    },
                },
                subsea::ir::Instruction::Assign {
                    dst: register("v3"),
                    value: subsea::ir::Value::FloatBinary {
                        width: subsea::ast::MemoryWidth::F64,
                        op: subsea::ast::FloatMathOp::Multiply,
                        lhs: subsea::ir::Operand::FloatLiteral("1.5".to_owned()),
                        rhs: register("v4"),
                    },
                },
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("fadd s0, s16, s17"));
    assert!(asm.contains("fmul d3, d16, d17"));
    assert!(asm.contains(".double 1.5"));
}

#[test]
fn aarch64_emits_loads_and_stores_that_assemble() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::TargetRegister("x0".to_owned()),
                    value: subsea::ir::Value::Operand(subsea::ir::Operand::Immediate(7)),
                },
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::Memory {
                        address: subsea::ir::Address {
                            first: subsea::ir::AddressTerm::TargetRegister("x1".to_owned()),
                            rest: Vec::new(),
                        },
                        width: Some(subsea::ast::MemoryWidth::U64),
                    },
                    value: subsea::ir::Value::Operand(subsea::ir::Operand::TargetRegister(
                        "x0".to_owned(),
                    )),
                },
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::TargetRegister("x0".to_owned()),
                    value: subsea::ir::Value::Operand(subsea::ir::Operand::Memory {
                        address: subsea::ir::Address {
                            first: subsea::ir::AddressTerm::TargetRegister("x1".to_owned()),
                            rest: Vec::new(),
                        },
                        width: Some(subsea::ast::MemoryWidth::U64),
                    }),
                },
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };
    let asm = subsea::backend::aarch64::emit(&program).unwrap();
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
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout {
                slots: vec![subsea::ir::StackSlot::Scalar {
                    name: "local".to_owned(),
                    width: subsea::ast::MemoryWidth::U64,
                }],
            },
            instructions: vec![
                subsea::ir::Instruction::Stack {
                    name: "local".to_owned(),
                    width: subsea::ast::MemoryWidth::U64,
                    value: subsea::ir::Operand::Immediate(9),
                },
                subsea::ir::Instruction::Ret,
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("  sub sp, sp, #64\n"));
    assert!(asm.contains("  stp x29, x30, [sp]\n  mov x29, sp\n"));
    assert!(asm.contains("  str x16, [x29, #48]\n"));
    assert!(asm.contains("  ldp x29, x30, [sp]\n  add sp, sp, #64\n  ret\n"));
}

#[test]
fn aarch64_emits_linux_write_runtime_operation() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Runtime(subsea::ir::RuntimeOperation::Print {
                    parts: vec![subsea::ir::PrintPart::Literal("hi\n".to_owned())],
                }),
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("mov x8, #64"));
    assert!(asm.contains("mov x2, #3"));
    assert!(asm.contains("svc #0"));
}

#[test]
fn aarch64_emits_linux_memory_runtime_operations() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::TargetRegister("x20".to_owned()),
                    value: subsea::ir::Value::PlatformReserve {
                        len: subsea::ir::Operand::Immediate(4096),
                    },
                },
                subsea::ir::Instruction::Runtime(subsea::ir::RuntimeOperation::Read {
                    source: subsea::ir::ReadSource::Stdin,
                    dst: subsea::ir::Operand::TargetRegister("x20".to_owned()),
                    len: subsea::ir::Operand::Immediate(16),
                }),
                subsea::ir::Instruction::Runtime(subsea::ir::RuntimeOperation::Release {
                    ptr: subsea::ir::Operand::TargetRegister("x20".to_owned()),
                    len: subsea::ir::Operand::Immediate(4096),
                }),
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("mov x8, #222"));
    assert!(asm.contains("mov x8, #63"));
    assert!(asm.contains("mov x8, #215"));
}

#[test]
fn aarch64_resolves_stack_operands_in_linux_memory_operations() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout {
                slots: vec![
                    subsea::ir::StackSlot::Scalar {
                        name: "ptr".to_owned(),
                        width: subsea::ast::MemoryWidth::Ptr,
                    },
                    subsea::ir::StackSlot::Scalar {
                        name: "len".to_owned(),
                        width: subsea::ast::MemoryWidth::U64,
                    },
                ],
            },
            instructions: vec![
                subsea::ir::Instruction::Runtime(subsea::ir::RuntimeOperation::Read {
                    source: subsea::ir::ReadSource::Stdin,
                    dst: subsea::ir::Operand::Name("ptr".to_owned()),
                    len: subsea::ir::Operand::Name("len".to_owned()),
                }),
                subsea::ir::Instruction::Assign {
                    dst: subsea::ir::Operand::TargetRegister("x0".to_owned()),
                    value: subsea::ir::Value::PlatformReserve {
                        len: subsea::ir::Operand::Name("len".to_owned()),
                    },
                },
                subsea::ir::Instruction::Runtime(subsea::ir::RuntimeOperation::Release {
                    ptr: subsea::ir::Operand::Name("ptr".to_owned()),
                    len: subsea::ir::Operand::Name("len".to_owned()),
                }),
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("mov x1, x29\n  add x1, x1, #48"));
    assert!(asm.contains("ldr x2, [x29, #56]"));
    assert!(asm.contains("ldr x1, [x29, #56]"));
    assert!(asm.contains("ldr x0, [x29, #48]"));
}

#[test]
fn aarch64_emits_stack_string_runtime_printing() {
    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout {
                slots: vec![subsea::ir::StackSlot::String {
                    name: "message".to_owned(),
                }],
            },
            instructions: vec![
                subsea::ir::Instruction::StackString {
                    name: "message".to_owned(),
                    value: subsea::ir::StringInitializer::Literal("hello\n".to_owned()),
                },
                subsea::ir::Instruction::Runtime(subsea::ir::RuntimeOperation::Print {
                    parts: vec![subsea::ir::PrintPart::Binding("message".to_owned())],
                }),
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };

    let asm = subsea::backend::aarch64::emit(&program).unwrap();

    assert!(asm.contains("ldr x1, [x29, #48]"));
    assert!(asm.contains("ldr x2, [x29, #56]"));
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

    let program = subsea::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![subsea::ir::Label {
            name: "main".to_owned(),
            stack: subsea::ir::StackLayout { slots: Vec::new() },
            instructions: vec![
                subsea::ir::Instruction::Runtime(subsea::ir::RuntimeOperation::Print {
                    parts: vec![subsea::ir::PrintPart::Literal("qemu\n".to_owned())],
                }),
                subsea::ir::Instruction::Exit { code: 0 },
            ],
        }],
    };
    let asm = subsea::backend::aarch64::emit(&program).unwrap();
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
