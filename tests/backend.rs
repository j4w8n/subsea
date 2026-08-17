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
        ("x86_64", Target::X86_64),
        ("x86_64-free", Target::X86_64Free),
    ] {
        assert_eq!(Target::parse(name), Ok(expected));
        assert_eq!(expected.name(), name);
    }
}

#[test]
fn aarch64_linux_target_describes_the_initial_cross_backend_contract() {
    let target = Target::AArch64Linux;
    let spec = target.spec();

    assert_eq!(target.name(), "aarch64-linux");
    assert_eq!(spec.architecture, Architecture::AArch64);
    assert_eq!(spec.environment, Environment::Linux);
    assert_eq!(spec.pointer_width, 64);
    assert_eq!(spec.pointer_alignment, 8);
    assert_eq!(spec.integer_argument_registers[0], "x0");
    assert_eq!(spec.integer_return_register, "x0");
    assert_eq!(spec.float_return_register, "v0");
    assert_eq!(spec.runtime_call_convention, "aapcs64");
    assert!(Target::parse("aarch64-linux").is_ok());
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

    assert!(asm.contains("  sub sp, sp, #16\n"));
    assert!(asm.contains("  str x16, [sp]\n"));
    assert!(asm.contains("  add sp, sp, #16\n  ret\n"));
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
