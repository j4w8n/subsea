use subsea::ast::{
    AssignmentTarget, AssignmentValue, Condition, ConditionExpr, ExprOp, Expression, Instruction,
    MathOp, MemoryWidth, Operand, Program,
};
use subsea::ir;
use subsea::lower::lower_program;

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
        labels: vec![subsea::ast::Label {
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
            target: subsea::ast::ControlTarget::Label(".done".to_owned()),
            condition: Some(ConditionExpr::Compare(Condition {
                lhs: ident("result"),
                op: subsea::ast::CompareOp::Equal,
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
            value: subsea::ast::StringInitializer::Slice {
                ptr: Operand::AddressOf(subsea::ast::Address {
                    first: subsea::ast::AddressTerm::Ident("buffer".to_owned()),
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
            parts: vec![subsea::ast::PrintPart::Literal("hello".to_owned())],
        },
        Instruction::Read {
            src: subsea::ast::ReadSource::Stdin,
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
