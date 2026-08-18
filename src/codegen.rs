//! Target dispatch and shared diagnostic validation.

use crate::ast::{AssignmentTarget, AssignmentValue, InlineAsmArchitecture, Instruction, Program};
use crate::backend::Architecture;
use crate::diagnostic::{Diagnostic, ProgramOrigins};
use crate::lower;
use crate::parser::validate_program_symbols;
use std::collections::HashSet;

pub use crate::backend::Target;

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
    )
}

fn inline_asm_name(architecture: InlineAsmArchitecture) -> &'static str {
    match architecture {
        InlineAsmArchitecture::X86_64 => "x86",
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

    match target.spec().architecture {
        Architecture::X86_64 => crate::backend::x86_64_codegen::emit_x86_64_asm_with_origins(
            program,
            target,
            entry_symbol,
            origins,
        )
        .map_err(|error| render_backend_diagnostic(&error, origins)),
        Architecture::AArch64 => {
            let semantic_ir = lower::lower_program(program).map_err(|error| {
                at_instruction(
                    Diagnostic::new(error.message),
                    origins,
                    &error.label,
                    error.instruction,
                )
            })?;

            crate::backend::aarch64::emit(&semantic_ir)
                .map_err(|error| render_backend_diagnostic(&error, origins))
        }
    }
}

fn render_backend_diagnostic(
    error: &crate::backend::BackendError,
    origins: &ProgramOrigins,
) -> Diagnostic {
    match (&error.label, error.instruction) {
        (Some(label), Some(index)) => {
            at_instruction(Diagnostic::new(&error.message), origins, label, index)
        }
        _ => Diagnostic::new(&error.message),
    }
}

pub fn emit_x86_64_asm(program: &Program, target: Target) -> Result<String, String> {
    crate::backend::x86_64_codegen::emit_x86_64_asm(program, target)
}

pub fn emit_x86_64_asm_with_entry_symbol(
    program: &Program,
    target: Target,
    entry_symbol: &str,
) -> Result<String, String> {
    crate::backend::x86_64_codegen::emit_x86_64_asm_with_entry_symbol(program, target, entry_symbol)
}

pub fn emit_x86_64_asm_with_origins(
    program: &Program,
    target: Target,
    entry_symbol: &str,
    origins: &ProgramOrigins,
) -> Result<String, Diagnostic> {
    validate_program_with_diagnostics_for_target(program, origins, target)?;
    crate::backend::x86_64_codegen::emit_x86_64_asm_with_origins(
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
