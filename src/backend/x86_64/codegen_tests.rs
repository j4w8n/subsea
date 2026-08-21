use crate::backend::Target;

fn ir_program(instructions: Vec<crate::ir::Instruction>) -> crate::ir::Program {
    crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions,
        }],
    }
}

#[test]
fn emits_private_integer_ir_assignment() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Assign {
                dst: crate::ir::Operand::TargetRegister("rax".to_owned()),
                value: crate::ir::Value::Operand(crate::ir::Operand::Immediate(7)),
            }],
        }],
    };

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("mov rax, 7\n"));
}

#[test]
fn emits_private_pair_arithmetic_ir() {
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
                dst: pair("rdx", "rax"),
                op: crate::ast::PairBinaryOp::Add,
                lhs: pair("rdx", "rax"),
                rhs: pair("rcx", "rbx"),
            }],
        }],
    };

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("add rax, rbx\n  adc rdx, rcx\n"));
}

#[test]
fn emits_private_integer_float_cast_ir() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Assign {
                dst: crate::ir::Operand::TargetRegister("xmm0".to_owned()),
                value: crate::ir::Value::Operand(crate::ir::Operand::Cast {
                    operand: Box::new(crate::ir::Operand::TargetRegister("rax".to_owned())),
                    width: crate::ast::MemoryWidth::F64,
                }),
            }],
        }],
    };

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("cvtsi2sd xmm0, rax\n"));
}

#[test]
fn rejects_private_narrow_indirect_call_target() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Call {
                target: crate::ir::ControlTarget::Operand(crate::ir::Operand::TargetRegister(
                    "eax".to_owned(),
                )),
            }],
        }],
    };

    let error = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap_err();

    assert_eq!(
        error.message,
        "indirect call target must be 64-bit, found 32-bit operand"
    );
}

#[test]
fn emits_private_address_of_ir() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Assign {
                dst: crate::ir::Operand::TargetRegister("rax".to_owned()),
                value: crate::ir::Value::Operand(crate::ir::Operand::AddressOf(
                    crate::ir::Address {
                        first: crate::ir::AddressTerm::Name("buffer".to_owned()),
                        rest: Vec::new(),
                    },
                )),
            }],
        }],
    };

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("buffer"));
}

#[test]
fn preserves_private_power_scratch_operands() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::Assign {
                dst: crate::ir::Operand::TargetRegister("rax".to_owned()),
                value: crate::ir::Value::Expression {
                    op: crate::ast::ExprOp::Power,
                    lhs: Box::new(crate::ir::Value::Operand(
                        crate::ir::Operand::TargetRegister("rbx".to_owned()),
                    )),
                    rhs: Box::new(crate::ir::Value::Operand(
                        crate::ir::Operand::TargetRegister("r10".to_owned()),
                    )),
                },
            }],
        }],
    };

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("mov r11, r10\n"));
    assert!(asm.contains("mov r10, rbx\n"));
}

#[test]
fn materializes_private_wide_rhs_that_uses_rax() {
    let program = crate::ir::Program {
        entry: "main".to_owned(),
        data: Vec::new(),
        memory: Vec::new(),
        labels: vec![crate::ir::Label {
            name: "main".to_owned(),
            stack: crate::ir::StackLayout { slots: Vec::new() },
            instructions: vec![crate::ir::Instruction::WideAssign {
                dst: crate::ir::RegisterPair {
                    high: "rdx".to_owned(),
                    low: "rax".to_owned(),
                },
                signed: false,
                division: false,
                lhs: crate::ir::Operand::TargetRegister("rbx".to_owned()),
                rhs: crate::ir::Operand::TargetRegister("rax".to_owned()),
            }],
        }],
    };

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("mov r10, rax\n"));
    assert!(asm.contains("mul r10\n"));
}

#[test]
fn emits_private_signed_wide_division() {
    let program = ir_program(vec![crate::ir::Instruction::WideAssign {
        dst: crate::ir::RegisterPair {
            high: "rdx".to_owned(),
            low: "rax".to_owned(),
        },
        signed: true,
        division: true,
        lhs: crate::ir::Operand::TargetRegister("rbx".to_owned()),
        rhs: crate::ir::Operand::TargetRegister("rcx".to_owned()),
    }]);

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("cqo\n"));
    assert!(asm.contains("idiv rcx\n"));
}

#[test]
fn emits_private_float_binary_widths() {
    let program = ir_program(vec![crate::ir::Instruction::Assign {
        dst: crate::ir::Operand::TargetRegister("xmm0".to_owned()),
        value: crate::ir::Value::FloatBinary {
            width: crate::ast::MemoryWidth::F32,
            op: crate::ast::FloatMathOp::Add,
            lhs: crate::ir::Operand::TargetRegister("xmm1".to_owned()),
            rhs: crate::ir::Operand::TargetRegister("xmm2".to_owned()),
        },
    }]);

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("addss xmm0, xmm2\n"));
}

#[test]
fn rejects_private_pair_destination_overlap() {
    let pair = |high: &str, low: &str| crate::ir::RegisterPair {
        high: high.to_owned(),
        low: low.to_owned(),
    };
    let program = ir_program(vec![crate::ir::Instruction::PairAssign {
        dst: pair("rax", "rax"),
        op: crate::ast::PairBinaryOp::Add,
        lhs: pair("rax", "rax"),
        rhs: pair("rcx", "rbx"),
    }]);

    let error = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap_err();

    assert!(
        error
            .message
            .contains("Pair arithmetic destination registers must be different")
    );
}

#[test]
fn emits_private_bitwise_arithmetic() {
    let program = ir_program(vec![crate::ir::Instruction::Assign {
        dst: crate::ir::Operand::TargetRegister("rax".to_owned()),
        value: crate::ir::Value::Binary {
            op: crate::ast::MathOp::BitAnd,
            lhs: crate::ir::Operand::TargetRegister("rax".to_owned()),
            rhs: crate::ir::Operand::TargetRegister("rbx".to_owned()),
        },
    }]);

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("and rax, rbx\n"));
}

#[test]
fn emits_private_float_to_integer_cast() {
    let program = ir_program(vec![crate::ir::Instruction::Assign {
        dst: crate::ir::Operand::TargetRegister("rax".to_owned()),
        value: crate::ir::Value::Operand(crate::ir::Operand::Cast {
            operand: Box::new(crate::ir::Operand::TargetRegister("xmm0".to_owned())),
            width: crate::ast::MemoryWidth::I64,
        }),
    }]);

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("cvttsd2si rax, xmm0\n"));
}

#[test]
fn emits_private_indexed_address() {
    let program = ir_program(vec![crate::ir::Instruction::Assign {
        dst: crate::ir::Operand::TargetRegister("rax".to_owned()),
        value: crate::ir::Value::Operand(crate::ir::Operand::AddressOf(crate::ir::Address {
            first: crate::ir::AddressTerm::TargetRegister("rbx".to_owned()),
            rest: vec![(
                crate::ir::AddressOperator::Add,
                crate::ir::AddressTerm::TargetRegister("rcx".to_owned()),
            )],
        })),
    }]);

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("lea rax, [rbx + rcx]\n"));
}

#[test]
fn emits_private_float_sqrt_intrinsic() {
    let program = ir_program(vec![crate::ir::Instruction::Assign {
        dst: crate::ir::Operand::TargetRegister("xmm0".to_owned()),
        value: crate::ir::Value::IntrinsicCall {
            op: crate::ast::IntrinsicOp::Sqrt,
            width: crate::ast::MemoryWidth::F64,
            args: vec![crate::ir::Operand::TargetRegister("xmm1".to_owned())],
        },
    }]);

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("sqrtsd xmm0, xmm1\n"));
}

#[test]
fn emits_private_exit_runtime_operation() {
    let program = ir_program(vec![crate::ir::Instruction::Exit { code: 7 }]);

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("mov rdi, 7\n"));
    assert!(asm.contains("mov rax, 60\n  syscall\n"));
}

#[test]
fn preserves_private_float_compare_registers() {
    let program = ir_program(vec![
        crate::ir::Instruction::Const {
            name: "one".to_owned(),
            value: crate::ir::ConstValue::Float {
                value: "1.0".to_owned(),
                width: crate::ast::MemoryWidth::F64,
            },
        },
        crate::ir::Instruction::AssignIf {
            dst: crate::ir::Operand::TargetRegister("rax".to_owned()),
            value: crate::ir::Value::Operand(crate::ir::Operand::Immediate(1)),
            condition: crate::ir::Condition::Compare {
                lhs: crate::ir::Operand::TargetRegister("xmm15".to_owned()),
                op: crate::ast::CompareOp::FloatEqual(crate::ast::MemoryWidth::F64),
                rhs: crate::ir::Operand::Name("one".to_owned()),
            },
        },
    ]);

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("ucomisd xmm15, qword ptr [rip + .Lfloatval_main_one]\n"));
    assert!(!asm.contains("xmm14"));
}

#[test]
fn emits_private_boolean_setcc_result() {
    let program = ir_program(vec![crate::ir::Instruction::Assign {
        dst: crate::ir::Operand::TargetRegister("rax".to_owned()),
        value: crate::ir::Value::Condition(crate::ir::Condition::Compare {
            lhs: crate::ir::Operand::TargetRegister("rbx".to_owned()),
            op: crate::ast::CompareOp::Equal,
            rhs: crate::ir::Operand::Immediate(0),
        }),
    }]);

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("test rbx, rbx\n"));
    assert!(asm.contains("sete r10b\n"));
    assert!(asm.contains("movzx rax, r10b\n"));
}

#[test]
fn rejects_private_release_abi_register_conflict() {
    let program = ir_program(vec![crate::ir::Instruction::Runtime(
        crate::ir::RuntimeOperation::Release {
            ptr: crate::ir::Operand::TargetRegister("rax".to_owned()),
            len: crate::ir::Operand::TargetRegister("rdi".to_owned()),
        },
    )]);

    let error = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap_err();

    assert_eq!(
        error.message,
        "release size cannot use rdi because release uses rdi for the pointer"
    );
}

#[test]
fn emits_private_scaled_address_lowering() {
    let program = ir_program(vec![crate::ir::Instruction::Assign {
        dst: crate::ir::Operand::TargetRegister("rax".to_owned()),
        value: crate::ir::Value::Operand(crate::ir::Operand::AddressOf(crate::ir::Address {
            first: crate::ir::AddressTerm::TargetRegister("rbx".to_owned()),
            rest: vec![(
                crate::ir::AddressOperator::Add,
                crate::ir::AddressTerm::ScaledTargetRegister {
                    register: "rcx".to_owned(),
                    scale: 8,
                },
            )],
        })),
    }]);

    let asm = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap();

    assert!(asm.contains("lea rax, [rbx + rcx * 8]\n"));
}

#[test]
fn rejects_private_float_memory_width_mismatch() {
    let program = ir_program(vec![crate::ir::Instruction::AssignIf {
        dst: crate::ir::Operand::TargetRegister("rax".to_owned()),
        value: crate::ir::Value::Operand(crate::ir::Operand::Immediate(1)),
        condition: crate::ir::Condition::Compare {
            lhs: crate::ir::Operand::Memory {
                address: crate::ir::Address {
                    first: crate::ir::AddressTerm::Name("left".to_owned()),
                    rest: Vec::new(),
                },
                width: Some(crate::ast::MemoryWidth::F32),
            },
            op: crate::ast::CompareOp::Equal,
            rhs: crate::ir::Operand::Memory {
                address: crate::ir::Address {
                    first: crate::ir::AddressTerm::Name("right".to_owned()),
                    rest: Vec::new(),
                },
                width: Some(crate::ast::MemoryWidth::F64),
            },
        },
    }]);

    let error = super::emit_ir_x86_64_asm(&program, Target::X86_64, "_start").unwrap_err();

    assert_eq!(
        error.message,
        "Floating-point comparison operands must have matching widths"
    );
}
