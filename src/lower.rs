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
        data: program.data.iter().map(lower_data_declaration).collect(),
        memory: program
            .memory
            .iter()
            .map(lower_memory_declaration)
            .collect(),
        labels,
    })
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
            Instruction::StackBuffer { name, count } => Some(ir::StackSlot::Buffer {
                name: name.clone(),
                count: *count,
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
        stack: lower_stack_layout(label),
        instructions,
    })
}

fn lower_data_declaration(data: &crate::ast::DataDeclaration) -> ir::DataDeclaration {
    ir::DataDeclaration {
        name: data.name.clone(),
        section: data.section.clone(),
        align: data.align,
        export: data.export,
        keep: data.keep,
        items: data
            .items
            .iter()
            .map(|item| match item {
                crate::ast::DataItem::Scalar { width, value } => ir::DataItem::Scalar {
                    width: *width,
                    value: *value,
                },
                crate::ast::DataItem::Addr { target } => ir::DataItem::Address {
                    target: target.clone(),
                },
                crate::ast::DataItem::Zero { count } => ir::DataItem::Zero { count: *count },
                crate::ast::DataItem::Label { name } => ir::DataItem::Label { name: name.clone() },
            })
            .collect(),
    }
}

fn lower_memory_declaration(memory: &crate::ast::MemoryDeclaration) -> ir::MemoryDeclaration {
    match memory {
        crate::ast::MemoryDeclaration::Aligned { declaration, align } => {
            ir::MemoryDeclaration::Aligned {
                declaration: Box::new(lower_memory_declaration(declaration)),
                align: *align,
            }
        }
        crate::ast::MemoryDeclaration::Scalar { name, width, value } => {
            ir::MemoryDeclaration::Scalar {
                name: name.clone(),
                width: *width,
                value: *value,
            }
        }
        crate::ast::MemoryDeclaration::FloatScalar { name, width, value } => {
            ir::MemoryDeclaration::FloatScalar {
                name: name.clone(),
                width: *width,
                value: value.clone(),
            }
        }
        crate::ast::MemoryDeclaration::Buffer { name, width, count } => {
            ir::MemoryDeclaration::Buffer {
                name: name.clone(),
                width: *width,
                count: *count,
            }
        }
        crate::ast::MemoryDeclaration::Array {
            name,
            width,
            values,
        } => ir::MemoryDeclaration::Array {
            name: name.clone(),
            width: *width,
            values: values.iter().map(lower_memory_value).collect(),
        },
        crate::ast::MemoryDeclaration::Repeat {
            name,
            width,
            count,
            value,
        } => ir::MemoryDeclaration::Repeat {
            name: name.clone(),
            width: *width,
            count: *count,
            value: lower_memory_value(value),
        },
    }
}

fn lower_memory_value(value: &crate::ast::MemoryValue) -> ir::MemoryValue {
    match value {
        crate::ast::MemoryValue::Integer(value) => ir::MemoryValue::Integer(*value),
        crate::ast::MemoryValue::Addr { target } => ir::MemoryValue::Address {
            target: target.clone(),
        },
    }
}

fn lower_instruction(
    label: &str,
    index: usize,
    instruction: &Instruction,
) -> Result<ir::Instruction, LoweringError> {
    match instruction {
        Instruction::Assign { dst, value } => match (dst, value) {
            (AssignmentTarget::RegisterPair(dst), AssignmentValue::PairBinary { op, lhs, rhs }) => {
                Ok(ir::Instruction::PairAssign {
                    dst: lower_pair(dst),
                    op: *op,
                    lhs: lower_pair(lhs),
                    rhs: lower_pair(rhs),
                })
            }
            (
                AssignmentTarget::RegisterPair(dst),
                AssignmentValue::WideMultiply { signed, lhs, rhs },
            ) => Ok(ir::Instruction::WideAssign {
                dst: lower_pair(dst),
                signed: *signed,
                division: false,
                lhs: lower_operand(lhs),
                rhs: lower_operand(rhs),
            }),
            (
                AssignmentTarget::RegisterPair(dst),
                AssignmentValue::WideDivide { signed, lhs, rhs },
            ) => Ok(ir::Instruction::WideAssign {
                dst: lower_pair(dst),
                signed: *signed,
                division: true,
                lhs: lower_operand(lhs),
                rhs: lower_operand(rhs),
            }),
            (dst, value) => Ok(ir::Instruction::Assign {
                dst: lower_target(dst, label, index)?,
                value: lower_value(value, label, index)?,
            }),
        },
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
        Instruction::StackBuffer { name, count } => Ok(ir::Instruction::StackBuffer {
            name: name.clone(),
            count: *count,
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
        Instruction::InlineAsm { architecture, text } => Ok(ir::Instruction::InlineAsm {
            architecture: *architecture,
            text: text.clone(),
        }),
        Instruction::Pop { dst } => Ok(ir::Instruction::Pop {
            dst: lower_operand(dst),
        }),
        Instruction::Push { src } => Ok(ir::Instruction::Push {
            src: lower_operand(src),
        }),
        Instruction::Syscall => Ok(ir::Instruction::Syscall),
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

fn lower_pair(pair: &crate::ast::RegisterPair) -> ir::RegisterPair {
    ir::RegisterPair {
        high: pair.high.clone(),
        low: pair.low.clone(),
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

#[cfg(test)]
mod tests {
    use crate::ast::{
        AssignmentTarget, AssignmentValue, Condition, ConditionExpr, ExprOp, Expression,
        Instruction, MathOp, MemoryWidth, Operand, Program,
    };
    use crate::ir;
    use crate::lower::lower_program;

    fn ident(name: &str) -> Operand {
        Operand::Ident(name.to_owned())
    }

    fn program(instructions: Vec<Instruction>) -> Program {
        Program {
            entry: "main".to_owned(),
            imports: Vec::new(),
            exports: Vec::new(),
            data: Vec::new(),
            memory: Vec::new(),
            labels: vec![crate::ast::Label {
                name: "main".to_owned(),
                instructions,
            }],
        }
    }

    #[test]
    fn lowers_core_assignment_and_control_flow() {
        let source = program(vec![
            Instruction::Assign {
                dst: AssignmentTarget::Operand(ident("result")),
                value: AssignmentValue::Binary {
                    op: MathOp::Add,
                    lhs: Operand::Immediate(2),
                    rhs: Operand::Immediate(3),
                },
            },
            Instruction::Jmp {
                target: crate::ast::ControlTarget::Label(".done".to_owned()),
                condition: Some(ConditionExpr::Compare(Condition {
                    lhs: ident("result"),
                    op: crate::ast::CompareOp::Equal,
                    rhs: Operand::Immediate(5),
                })),
            },
            Instruction::Label {
                name: ".done".to_owned(),
            },
            Instruction::Ret,
        ]);

        let lowered = lower_program(&source).unwrap();

        assert_eq!(lowered.entry, "main");
        assert_eq!(lowered.labels.len(), 1);
        assert!(matches!(
            &lowered.labels[0].instructions[0],
            ir::Instruction::Assign {
                dst: ir::Operand::Name(name),
                value: ir::Value::Binary { op: MathOp::Add, .. }
            } if name == "result"
        ));
        assert!(matches!(
            &lowered.labels[0].instructions[1],
            ir::Instruction::Jmp {
                target: ir::ControlTarget::Label(name),
                condition: Some(ir::Condition::Compare { .. })
            } if name == ".done"
        ));
    }

    #[test]
    fn lowers_nested_expression_trees_without_selecting_machine_instructions() {
        let source = program(vec![
            Instruction::Assign {
                dst: AssignmentTarget::Operand(ident("result")),
                value: AssignmentValue::Expression(Expression::Binary {
                    op: ExprOp::Math(MathOp::Multiply),
                    lhs: Box::new(Expression::Binary {
                        op: ExprOp::Math(MathOp::Add),
                        lhs: Box::new(Expression::Operand(Operand::Immediate(2))),
                        rhs: Box::new(Expression::Operand(Operand::Immediate(3))),
                    }),
                    rhs: Box::new(Expression::Operand(Operand::Immediate(4))),
                }),
            },
            Instruction::Ret,
        ]);

        let lowered = lower_program(&source).unwrap();

        assert!(matches!(
            &lowered.labels[0].instructions[0],
            ir::Instruction::Assign {
                value: ir::Value::Expression {
                    op: ExprOp::Math(MathOp::Multiply),
                    lhs,
                    rhs,
                },
                ..
            } if matches!(lhs.as_ref(), ir::Value::Expression {
                op: ExprOp::Math(MathOp::Add), ..
            }) && matches!(rhs.as_ref(), ir::Value::Operand(ir::Operand::Immediate(4)))
        ));
    }

    #[test]
    fn lowers_stack_strings_without_an_architecture_specific_frame() {
        let source = program(vec![
            Instruction::StackString {
                name: "message".to_owned(),
                value: crate::ast::StringInitializer::Slice {
                    ptr: Operand::AddressOf(crate::ast::Address {
                        first: crate::ast::AddressTerm::Ident("buffer".to_owned()),
                        rest: Vec::new(),
                    }),
                    len: Operand::Immediate(4),
                },
            },
            Instruction::Stack {
                name: "count".to_owned(),
                width: MemoryWidth::U64,
                value: Operand::Immediate(4),
            },
            Instruction::Ret,
        ]);

        let lowered = lower_program(&source).unwrap();

        assert!(matches!(
            &lowered.labels[0].instructions[0],
            ir::Instruction::StackString {
                value: ir::StringInitializer::Slice {
                    ptr: ir::Operand::AddressOf(_),
                    len: ir::Operand::Immediate(4),
                },
                ..
            }
        ));
        assert!(matches!(
            &lowered.labels[0].instructions[1],
            ir::Instruction::Stack {
                width: MemoryWidth::U64,
                value: ir::Operand::Immediate(4),
                ..
            }
        ));
    }

    #[test]
    fn reports_target_specific_instructions_at_their_source_location() {
        let source = program(vec![
            Instruction::Nop,
            Instruction::Push {
                src: Operand::Immediate(1),
            },
        ]);

        let lowered = lower_program(&source).unwrap();

        assert!(matches!(
            lowered.labels[0].instructions.get(1),
            Some(ir::Instruction::Push {
                src: ir::Operand::Immediate(1)
            })
        ));
    }

    #[test]
    fn lowers_runtime_and_platform_operations_without_machine_instructions() {
        let source = program(vec![
            Instruction::Print {
                parts: vec![crate::ast::PrintPart::Literal("hello".to_owned())],
            },
            Instruction::Read {
                src: crate::ast::ReadSource::Stdin,
                dst: ident("buffer"),
                len: Operand::Immediate(8),
            },
            Instruction::Release {
                ptr: ident("buffer"),
                len: Operand::Immediate(8),
            },
            Instruction::Assign {
                dst: AssignmentTarget::Operand(ident("address")),
                value: AssignmentValue::LinuxReserve {
                    len: Operand::Immediate(4096),
                },
            },
        ]);

        let lowered = lower_program(&source).unwrap();

        assert!(matches!(
            &lowered.labels[0].instructions[0],
            ir::Instruction::Runtime(ir::RuntimeOperation::Print { parts })
                if matches!(&parts[0], ir::PrintPart::Literal(value) if value == "hello")
        ));
        assert!(matches!(
            &lowered.labels[0].instructions[1],
            ir::Instruction::Runtime(ir::RuntimeOperation::Read {
                source: ir::ReadSource::Stdin,
                dst: ir::Operand::Name(name),
                len: ir::Operand::Immediate(8),
            }) if name == "buffer"
        ));
        assert!(matches!(
            &lowered.labels[0].instructions[2],
            ir::Instruction::Runtime(ir::RuntimeOperation::Release {
                ptr: ir::Operand::Name(name),
                len: ir::Operand::Immediate(8),
            }) if name == "buffer"
        ));
        assert!(matches!(
            &lowered.labels[0].instructions[3],
            ir::Instruction::Assign {
                value: ir::Value::PlatformReserve {
                    len: ir::Operand::Immediate(4096),
                },
                ..
            }
        ));
    }
}
