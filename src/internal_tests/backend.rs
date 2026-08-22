use crate::codegen::{
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
