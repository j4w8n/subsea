use crate::ast::{
    Address, AddressOperator, AddressTerm, AssignmentTarget, AssignmentValue, BindingValue,
    ConditionExpr, ControlTarget, Expression, Instruction, Operand, StringInitializer,
    StringProperty, WidthConversion,
};
use crate::ir;

#[derive(Debug, PartialEq, Eq)]
pub struct LoweringError {
    pub label: String,
    pub instruction: usize,
    pub message: String,
}

impl LoweringError {
    fn unsupported(label: &str, instruction: usize, feature: &str) -> Self {
        Self {
            label: label.to_owned(),
            instruction,
            message: format!("{feature} is not represented in the target-neutral IR yet"),
        }
    }
}

pub fn lower_program(program: &crate::ast::Program) -> Result<ir::Program, LoweringError> {
    let labels = program
        .labels
        .iter()
        .map(lower_label)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ir::Program {
        entry: program.entry.clone(),
        labels,
    })
}

pub fn lower_assignment(
    dst: &AssignmentTarget,
    value: &AssignmentValue,
    label: &str,
    instruction: usize,
) -> Result<ir::Instruction, LoweringError> {
    Ok(ir::Instruction::Assign {
        dst: lower_target(dst, label, instruction)?,
        value: lower_value(value, label, instruction)?,
    })
}

pub fn lower_runtime_instruction(
    instruction: &Instruction,
    label: &str,
    index: usize,
) -> Result<ir::Instruction, LoweringError> {
    lower_instruction(label, index, instruction)
}

pub fn lower_stack_layout(label: &crate::ast::Label) -> ir::StackLayout {
    let slots = label
        .instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::Stack { name, width, .. } => Some(ir::StackSlot::Scalar {
                name: name.clone(),
                width: *width,
            }),
            Instruction::StackString { name, .. } => {
                Some(ir::StackSlot::String { name: name.clone() })
            }
            _ => None,
        })
        .collect();

    ir::StackLayout { slots }
}

fn lower_label(label: &crate::ast::Label) -> Result<ir::Label, LoweringError> {
    let mut instructions = Vec::with_capacity(label.instructions.len());

    for (index, instruction) in label.instructions.iter().enumerate() {
        instructions.push(lower_instruction(&label.name, index, instruction)?);
    }

    Ok(ir::Label {
        name: label.name.clone(),
        instructions,
    })
}

fn lower_instruction(
    label: &str,
    index: usize,
    instruction: &Instruction,
) -> Result<ir::Instruction, LoweringError> {
    match instruction {
        Instruction::Assign { dst, value } => Ok(ir::Instruction::Assign {
            dst: lower_target(dst, label, index)?,
            value: lower_value(value, label, index)?,
        }),
        Instruction::AssignIf {
            dst,
            value,
            condition,
        } => Ok(ir::Instruction::AssignIf {
            dst: lower_target(dst, label, index)?,
            value: lower_value(value, label, index)?,
            condition: lower_condition(condition),
        }),
        Instruction::Const { name, value } => Ok(ir::Instruction::Const {
            name: name.clone(),
            value: lower_const_value(value),
        }),
        Instruction::Call { target } => Ok(ir::Instruction::Call {
            target: lower_control_target(target),
        }),
        Instruction::Exit { code } => Ok(ir::Instruction::Exit { code: *code }),
        Instruction::Jmp { target, condition } => Ok(ir::Instruction::Jmp {
            target: lower_control_target(target),
            condition: condition.as_ref().map(lower_condition),
        }),
        Instruction::Label { name } => Ok(ir::Instruction::Label { name: name.clone() }),
        Instruction::Nop => Ok(ir::Instruction::Nop),
        Instruction::Ret => Ok(ir::Instruction::Ret),
        Instruction::Stack { name, width, value } => Ok(ir::Instruction::Stack {
            name: name.clone(),
            width: *width,
            value: lower_operand(value),
        }),
        Instruction::StackString { name, value } => Ok(ir::Instruction::StackString {
            name: name.clone(),
            value: lower_string_initializer(value),
        }),
        Instruction::Print { parts } => Ok(ir::Instruction::Runtime(ir::RuntimeOperation::Print {
            parts: parts.iter().map(lower_print_part).collect(),
        })),
        Instruction::Read { src, dst, len } => {
            Ok(ir::Instruction::Runtime(ir::RuntimeOperation::Read {
                source: match src {
                    crate::ast::ReadSource::Stdin => ir::ReadSource::Stdin,
                },
                dst: lower_operand(dst),
                len: lower_operand(len),
            }))
        }
        Instruction::Release { ptr, len } => {
            Ok(ir::Instruction::Runtime(ir::RuntimeOperation::Release {
                ptr: lower_operand(ptr),
                len: lower_operand(len),
            }))
        }
        Instruction::InlineAsm { .. } => {
            Err(LoweringError::unsupported(label, index, "inline assembly"))
        }
        Instruction::Pop { .. } => Err(LoweringError::unsupported(label, index, "pop")),
        Instruction::Push { .. } => Err(LoweringError::unsupported(label, index, "push")),
        Instruction::Syscall => Err(LoweringError::unsupported(label, index, "raw syscalls")),
    }
}

fn lower_const_value(value: &BindingValue) -> ir::ConstValue {
    match value {
        BindingValue::Integer { value, width } => ir::ConstValue::Integer {
            value: *value,
            width: *width,
        },
        BindingValue::Float { value, width } => ir::ConstValue::Float {
            value: value.clone(),
            width: *width,
        },
        BindingValue::String(value) => ir::ConstValue::String(value.clone()),
    }
}

fn lower_target(
    target: &AssignmentTarget,
    label: &str,
    index: usize,
) -> Result<ir::Operand, LoweringError> {
    match target {
        AssignmentTarget::Operand(operand) => Ok(lower_operand(operand)),
        AssignmentTarget::RegisterPair(_) => Err(LoweringError::unsupported(
            label,
            index,
            "register-pair destinations",
        )),
    }
}

fn lower_value(
    value: &AssignmentValue,
    label: &str,
    index: usize,
) -> Result<ir::Value, LoweringError> {
    match value {
        AssignmentValue::Operand(operand) => Ok(ir::Value::Operand(lower_operand(operand))),
        AssignmentValue::Expression(expression) => lower_expression(expression, label, index),
        AssignmentValue::Binary { op, lhs, rhs } => Ok(ir::Value::Binary {
            op: *op,
            lhs: lower_operand(lhs),
            rhs: lower_operand(rhs),
        }),
        AssignmentValue::BitwiseUnary { op, operand } => Ok(ir::Value::BitwiseUnary {
            op: *op,
            operand: lower_operand(operand),
        }),
        AssignmentValue::Condition(condition) => {
            Ok(ir::Value::Condition(lower_condition(condition)))
        }
        AssignmentValue::FloatBinary {
            width,
            op,
            lhs,
            rhs,
        } => Ok(ir::Value::FloatBinary {
            width: *width,
            op: *op,
            lhs: lower_operand(lhs),
            rhs: lower_operand(rhs),
        }),
        AssignmentValue::IntrinsicCall { op, width, args } => Ok(ir::Value::IntrinsicCall {
            op: *op,
            width: *width,
            args: args
                .iter()
                .map(|arg| Ok(lower_operand(arg)))
                .collect::<Result<Vec<_>, LoweringError>>()?,
        }),
        AssignmentValue::StringBytes { value } => Ok(ir::Value::StringBytes {
            value: value.clone(),
        }),
        AssignmentValue::LinuxReserve { len } => Ok(ir::Value::PlatformReserve {
            len: lower_operand(len),
        }),
        AssignmentValue::WideMultiply { .. }
        | AssignmentValue::WideDivide { .. }
        | AssignmentValue::PairBinary { .. } => Err(LoweringError::unsupported(
            label,
            index,
            "target-specific assignment",
        )),
    }
}

fn lower_print_part(part: &crate::ast::PrintPart) -> ir::PrintPart {
    match part {
        crate::ast::PrintPart::Binding(name) => ir::PrintPart::Binding(name.clone()),
        crate::ast::PrintPart::FormattedOperand { format, operand } => {
            ir::PrintPart::FormattedOperand {
                format: match format {
                    crate::ast::PrintFormat::Infer => ir::PrintFormat::Infer,
                    crate::ast::PrintFormat::SignedDecimal(width) => {
                        ir::PrintFormat::SignedDecimal(*width)
                    }
                    crate::ast::PrintFormat::UnsignedDecimal(width) => {
                        ir::PrintFormat::UnsignedDecimal(*width)
                    }
                    crate::ast::PrintFormat::Hex => ir::PrintFormat::Hex,
                    crate::ast::PrintFormat::Binary => ir::PrintFormat::Binary,
                    crate::ast::PrintFormat::Pointer => ir::PrintFormat::Pointer,
                },
                operand: lower_operand(operand),
            }
        }
        crate::ast::PrintPart::Literal(value) => ir::PrintPart::Literal(value.clone()),
        crate::ast::PrintPart::Operand(operand) => ir::PrintPart::Operand(lower_operand(operand)),
    }
}

fn lower_expression(
    expression: &Expression,
    label: &str,
    index: usize,
) -> Result<ir::Value, LoweringError> {
    match expression {
        Expression::Operand(operand) => Ok(ir::Value::Operand(lower_operand(operand))),
        Expression::Binary { op, lhs, rhs } => Ok(ir::Value::Expression {
            op: *op,
            lhs: Box::new(lower_expression(lhs, label, index)?),
            rhs: Box::new(lower_expression(rhs, label, index)?),
        }),
    }
}

pub fn lower_condition(condition: &ConditionExpr) -> ir::Condition {
    match condition {
        ConditionExpr::Compare(condition) => ir::Condition::Compare {
            lhs: lower_operand(&condition.lhs),
            op: condition.op,
            rhs: lower_operand(&condition.rhs),
        },
        ConditionExpr::BitwiseAndZero { lhs, rhs, op } => ir::Condition::BitwiseAndZero {
            lhs: lower_operand(lhs),
            rhs: lower_operand(rhs),
            op: *op,
        },
    }
}

pub fn lower_control_target(target: &ControlTarget) -> ir::ControlTarget {
    match target {
        ControlTarget::Label(name) => ir::ControlTarget::Label(name.clone()),
        ControlTarget::Operand(operand) => ir::ControlTarget::Operand(lower_operand(operand)),
    }
}

fn lower_string_initializer(initializer: &StringInitializer) -> ir::StringInitializer {
    match initializer {
        StringInitializer::Literal(value) => ir::StringInitializer::Literal(value.clone()),
        StringInitializer::Slice { ptr, len } => ir::StringInitializer::Slice {
            ptr: lower_operand(ptr),
            len: lower_operand(len),
        },
    }
}

fn lower_operand(operand: &Operand) -> ir::Operand {
    match operand {
        Operand::Converted {
            operand,
            conversion,
        } => ir::Operand::Converted {
            operand: Box::new(lower_operand(operand)),
            conversion: match conversion {
                WidthConversion::SignExtend => ir::WidthConversion::SignExtend,
                WidthConversion::ZeroExtend => ir::WidthConversion::ZeroExtend,
            },
        },
        Operand::Cast { operand, width } => ir::Operand::Cast {
            operand: Box::new(lower_operand(operand)),
            width: *width,
        },
        Operand::Dereference { address, width } => ir::Operand::Memory {
            address: lower_address(address),
            width: *width,
        },
        Operand::AddressOf(address) => ir::Operand::AddressOf(lower_address(address)),
        Operand::FloatLiteral(value) => ir::Operand::FloatLiteral(value.clone()),
        Operand::Immediate(value) => ir::Operand::Immediate(*value),
        Operand::Register(name) => ir::Operand::TargetRegister(name.clone()),
        Operand::Ident(name) => ir::Operand::Name(name.clone()),
        Operand::StringProperty { name, property } => ir::Operand::StringProperty {
            name: name.clone(),
            property: match property {
                StringProperty::Len => ir::StringProperty::Len,
                StringProperty::Ptr => ir::StringProperty::Ptr,
            },
        },
        Operand::Pointer(name) => ir::Operand::Pointer(name.clone()),
    }
}

fn lower_address(address: &Address) -> ir::Address {
    ir::Address {
        first: lower_address_term(&address.first),
        rest: address
            .rest
            .iter()
            .map(|(operator, term)| {
                (
                    match operator {
                        AddressOperator::Add => ir::AddressOperator::Add,
                        AddressOperator::Subtract => ir::AddressOperator::Subtract,
                    },
                    lower_address_term(term),
                )
            })
            .collect(),
    }
}

fn lower_address_term(term: &AddressTerm) -> ir::AddressTerm {
    match term {
        AddressTerm::Immediate(value) => ir::AddressTerm::Immediate(*value),
        AddressTerm::Register(name) => ir::AddressTerm::TargetRegister(name.clone()),
        AddressTerm::ScaledRegister { register, scale } => ir::AddressTerm::ScaledTargetRegister {
            register: register.clone(),
            scale: *scale,
        },
        AddressTerm::Ident(name) => ir::AddressTerm::Name(name.clone()),
    }
}
