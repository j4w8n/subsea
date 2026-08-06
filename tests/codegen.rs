use subsea::ast::{
    Address, AddressTerm, AssignmentTarget, AssignmentValue, BindingValue, Instruction, Label,
    MathOp, MemoryDeclaration, MemoryWidth, Operand, PrintPart, Program,
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

#[test]
fn prints_integer_binding() {
    let program = main_program(vec![
        Instruction::Let {
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
fn uses_integer_binding_as_immediate_operand() {
    let program = main_program(vec![
        Instruction::Let {
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
fn formats_integer_binding() {
    let program = main_program(vec![
        Instruction::Let {
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
        Instruction::Let {
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
