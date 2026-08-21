//! Target dispatch and shared diagnostic validation.

use crate::ast::{AssignmentTarget, AssignmentValue, InlineAsmArchitecture, Instruction, Program};
use crate::diagnostic::{Diagnostic, ProgramOrigins};
use crate::lower;
use crate::parser::validate_program_symbols;
use std::collections::HashSet;

pub use crate::backend::{
    Architecture, BackendError, EntryConvention, Environment, FramePointerPolicy, RuntimeOperation,
    Target, TargetSpec,
};

pub fn validate_program_with_diagnostics_for_target(
    program: &Program,
    origins: &ProgramOrigins,
    target: Target,
) -> Result<(), Diagnostic> {
    validate_program_symbols(program).map_err(Diagnostic::new)?;
    validate_target_registers(program, target, origins)?;

    let top_level_labels: HashSet<&str> = program
        .labels
        .iter()
        .map(|label| label.name.as_str())
        .collect();
    for label in &program.labels {
        let stack = crate::analysis::build_stack_frame_from_layout(
            &lower::lower_stack_layout(label),
            target.spec().stack_alignment,
        );
        crate::analysis::validate_label(
            label,
            &top_level_labels,
            &stack,
            target.spec().frame_pointer,
            target.spec().exit_syscall,
        )
        .map_err(|message| at_instruction(Diagnostic::new(message), origins, &label.name, 0))?;
    }

    Ok(())
}

fn validate_target_registers(
    program: &Program,
    target: Target,
    origins: &ProgramOrigins,
) -> Result<(), Diagnostic> {
    for label in &program.labels {
        for (index, instruction) in label.instructions.iter().enumerate() {
            if let Instruction::InlineAsm { architecture, .. } = instruction
                && !inline_asm_matches_target(*architecture, target)
            {
                return Err(at_instruction(
                    Diagnostic::new(format!(
                        "{} inline assembly cannot be used with target {}",
                        inline_asm_name(*architecture),
                        target.name()
                    )),
                    origins,
                    &label.name,
                    index,
                ));
            }

            let mut invalid = None;
            instruction.visit_operands(|operand| {
                if invalid.is_some() {
                    return;
                }
                operand.visit_registers(|register| {
                    if invalid.is_none() && !target.is_register(register) {
                        invalid = Some(register.to_owned());
                    }
                });
            });

            if invalid.is_none() {
                match instruction {
                    Instruction::Assign {
                        dst: AssignmentTarget::RegisterPair(pair),
                        ..
                    }
                    | Instruction::AssignIf {
                        dst: AssignmentTarget::RegisterPair(pair),
                        ..
                    } => {
                        for register in [&pair.high, &pair.low] {
                            if !target.is_register(register) {
                                invalid = Some(register.clone());
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }

            if invalid.is_none()
                && let Instruction::Assign {
                    value: AssignmentValue::PairBinary { lhs, rhs, .. },
                    ..
                } = instruction
            {
                for register in [&lhs.high, &lhs.low, &rhs.high, &rhs.low] {
                    if !target.is_register(register) {
                        invalid = Some(register.clone());
                        break;
                    }
                }
            }

            if let Some(register) = invalid {
                return Err(at_instruction(
                    Diagnostic::new(format!(
                        "Register {register:?} is not available on target {}",
                        target.name()
                    )),
                    origins,
                    &label.name,
                    index,
                ));
            }
        }
    }

    Ok(())
}

fn inline_asm_matches_target(architecture: InlineAsmArchitecture, target: Target) -> bool {
    matches!(
        (architecture, target.spec().architecture),
        (InlineAsmArchitecture::X86_64, Architecture::X86_64)
            | (InlineAsmArchitecture::AArch64, Architecture::AArch64)
    )
}

fn inline_asm_name(architecture: InlineAsmArchitecture) -> &'static str {
    match architecture {
        InlineAsmArchitecture::X86_64 => "x86",
        InlineAsmArchitecture::AArch64 => "AArch64",
    }
}

fn at_instruction(
    diagnostic: Diagnostic,
    origins: &ProgramOrigins,
    label: &str,
    index: usize,
) -> Diagnostic {
    origins
        .instruction_span(label, index)
        .map_or(diagnostic.clone(), |span| diagnostic.at(span))
}

pub fn emit_target_asm_with_origins(
    program: &Program,
    target: Target,
    entry_symbol: &str,
    origins: &ProgramOrigins,
) -> Result<String, Diagnostic> {
    validate_program_with_diagnostics_for_target(program, origins, target)?;
    let semantic_ir = lower::lower_program(program).map_err(|error| {
        at_instruction(
            Diagnostic::new(error.message),
            origins,
            &error.label,
            error.instruction,
        )
    })?;

    match target.spec().architecture {
        Architecture::X86_64 => crate::backend::x86_64::codegen::emit_ir_x86_64_asm_with_origins(
            &semantic_ir,
            target,
            entry_symbol,
            origins,
        )
        .map_err(|error| render_backend_diagnostic(&error, origins)),
        Architecture::AArch64 => {
            crate::backend::aarch64::emit_for_target_with_entry(&semantic_ir, target, entry_symbol)
                .map_err(|error| render_backend_diagnostic(&error, origins))
        }
    }
}

#[cfg(test)]
fn emit_target_ir_asm(
    program: &crate::ir::Program,
    target: Target,
    entry_symbol: &str,
) -> Result<String, BackendError> {
    match target.spec().architecture {
        Architecture::X86_64 => {
            crate::backend::x86_64::codegen::emit_ir_x86_64_asm(program, target, entry_symbol)
        }
        Architecture::AArch64 => {
            crate::backend::aarch64::emit_for_target_with_entry(program, target, entry_symbol)
        }
    }
}

fn render_backend_diagnostic(error: &BackendError, origins: &ProgramOrigins) -> Diagnostic {
    match (&error.label, error.instruction) {
        (Some(label), Some(index)) => {
            at_instruction(Diagnostic::new(&error.message), origins, label, index)
        }
        _ => Diagnostic::new(&error.message),
    }
}

pub fn emit_x86_64_asm(program: &Program, target: Target) -> Result<String, String> {
    crate::backend::x86_64::codegen::emit_x86_64_asm(program, target)
}

pub fn emit_x86_64_asm_with_entry_symbol(
    program: &Program,
    target: Target,
    entry_symbol: &str,
) -> Result<String, String> {
    crate::backend::x86_64::codegen::emit_x86_64_asm_with_entry_symbol(
        program,
        target,
        entry_symbol,
    )
}

pub fn emit_x86_64_asm_with_origins(
    program: &Program,
    target: Target,
    entry_symbol: &str,
    origins: &ProgramOrigins,
) -> Result<String, Diagnostic> {
    validate_program_with_diagnostics_for_target(program, origins, target)?;
    crate::backend::x86_64::codegen::emit_x86_64_asm_with_origins(
        program,
        target,
        entry_symbol,
        origins,
    )
    .map_err(|error| render_backend_diagnostic(&error, origins))
}

pub fn emit_x86_64_linux_asm(program: &Program) -> Result<String, String> {
    emit_x86_64_asm(program, Target::X86_64)
}

#[cfg(test)]
mod aarch64_tests {
    use super::{Architecture, Environment, RuntimeOperation, Target};
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

        let error = super::emit_target_ir_asm(&program, Target::AArch64Free, "_start").unwrap_err();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Free, "kernel_entry").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

        assert!(asm.contains("sdiv x16, x16, x17"));
        assert!(asm.contains("udiv x18, x16, x17"));
        assert!(asm.contains("msub x16, x18, x17, x16"));
        assert!(asm.contains("mul x18, x18, x16"));
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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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
                    crate::ir::Instruction::Exit { code: 0 },
                ],
            }],
        };
        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();
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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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

        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();

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
        let asm = super::emit_target_ir_asm(&program, Target::AArch64Linux, "_start").unwrap();
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
}
