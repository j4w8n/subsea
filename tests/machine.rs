use subsea::backend::x86_64_machine::{self as machine, Instruction, Operand};

#[test]
fn emits_machine_move_without_assembly_selection_in_the_caller() {
    let instruction = Instruction::Move {
        dst: Operand::Register("rax".to_owned()),
        src: Operand::Address("qword ptr [value]".to_owned()),
    };
    let mut asm = String::new();

    machine::emit(&instruction, &mut asm);

    assert_eq!(asm, "  mov rax, qword ptr [value]\n");
}

#[test]
fn machine_binary_keeps_opcode_and_operands_structured() {
    let instruction = Instruction::Binary {
        opcode: "add".to_owned(),
        dst: Operand::Register("rax".to_owned()),
        src: Operand::Immediate(4),
    };
    let mut asm = String::new();

    machine::emit(&instruction, &mut asm);

    assert_eq!(asm, "  add rax, 4\n");
}

#[test]
fn emits_machine_call_and_return() {
    let mut asm = String::new();

    machine::emit(
        &Instruction::Call {
            target: Operand::Address("main".to_owned()),
        },
        &mut asm,
    );
    machine::emit(&Instruction::Return, &mut asm);

    assert_eq!(asm, "  call main\n  ret\n");
}

#[test]
fn emits_machine_branch() {
    let mut asm = String::new();

    machine::emit(
        &Instruction::Branch {
            opcode: "je".to_owned(),
            target: Operand::Address("done".to_owned()),
        },
        &mut asm,
    );

    assert_eq!(asm, "  je done\n");
}

#[test]
fn emits_machine_memory_and_stack_operations() {
    let instructions = [
        Instruction::Load {
            dst: Operand::Register("rax".to_owned()),
            src: Operand::Address("qword ptr [value]".to_owned()),
        },
        Instruction::Store {
            dst: Operand::Address("qword ptr [value]".to_owned()),
            src: Operand::Register("rax".to_owned()),
        },
        Instruction::Push {
            src: Operand::Register("rax".to_owned()),
        },
        Instruction::Pop {
            dst: Operand::Register("rax".to_owned()),
        },
    ];
    let mut asm = String::new();

    for instruction in &instructions {
        machine::emit(instruction, &mut asm);
    }

    assert_eq!(
        asm,
        "  mov rax, qword ptr [value]\n  mov qword ptr [value], rax\n  push rax\n  pop rax\n"
    );
}

#[test]
fn emits_machine_stack_adjust_and_syscall() {
    let instructions = [
        Instruction::StackAdjust {
            opcode: "sub".to_owned(),
            register: "rsp".to_owned(),
            amount: 32,
        },
        Instruction::Syscall { number: 60 },
    ];
    let mut asm = String::new();

    for instruction in &instructions {
        machine::emit(instruction, &mut asm);
    }

    assert_eq!(asm, "  sub rsp, 32\n  mov rax, 60\n  syscall\n");
}

#[test]
fn emits_machine_labels_nops_and_runtime_calls() {
    let instructions = [
        Instruction::Label {
            name: "loop".to_owned(),
        },
        Instruction::Nop,
        Instruction::RuntimeCall {
            target: Operand::Address("runtime_print".to_owned()),
        },
    ];
    let mut asm = String::new();

    for instruction in &instructions {
        machine::emit(instruction, &mut asm);
    }

    assert_eq!(asm, "loop:\n  nop\n  call runtime_print\n");
}

#[test]
fn emits_machine_compare() {
    let instruction = Instruction::Compare {
        opcode: "cmp".to_owned(),
        lhs: Operand::Register("rax".to_owned()),
        rhs: Operand::Immediate(0),
    };
    let mut asm = String::new();

    machine::emit(&instruction, &mut asm);

    assert_eq!(asm, "  cmp rax, 0\n");
}

#[test]
fn emits_structured_x86_memory_address() {
    let instruction = Instruction::Load {
        dst: Operand::Register("rax".to_owned()),
        src: Operand::Memory(machine::MemoryAddress {
            width: Some("qword".to_owned()),
            terms: vec![
                (
                    machine::AddressOperator::Add,
                    machine::AddressTerm::Symbol("buffer".to_owned()),
                ),
                (
                    machine::AddressOperator::Add,
                    machine::AddressTerm::ScaledRegister {
                        register: "rcx".to_owned(),
                        scale: 4,
                    },
                ),
            ],
        }),
    };
    let mut asm = String::new();

    machine::emit(&instruction, &mut asm);

    assert_eq!(asm, "  mov rax, qword ptr [buffer + rcx * 4]\n");
}
