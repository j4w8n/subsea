use subsea::backend::{Architecture, Environment, Target};

#[test]
fn x86_64_linux_target_describes_its_backend_properties() {
    let target = Target::X86_64;
    let spec = target.spec();

    assert_eq!(spec.architecture, Architecture::X86_64);
    assert_eq!(spec.environment, Environment::Linux);
    assert_eq!(spec.pointer_width, 64);
    assert_eq!(spec.linker_emulation, "elf_x86_64");
    assert_eq!(spec.stack_alignment, 16);
    assert_eq!(spec.stack_pointer, "rsp");
    assert_eq!(spec.frame_pointer, "rbp");
    assert!(!target.is_freestanding());
}

#[test]
fn x86_64_freestanding_target_shares_architecture_but_changes_environment() {
    let target = Target::X86_64Free;
    let spec = target.spec();

    assert_eq!(spec.architecture, Architecture::X86_64);
    assert_eq!(spec.environment, Environment::Freestanding);
    assert_eq!(spec.pointer_width, 64);
    assert_eq!(spec.linker_emulation, "elf_x86_64");
    assert_eq!(spec.stack_alignment, 16);
    assert_eq!(spec.stack_pointer, "rsp");
    assert_eq!(spec.frame_pointer, "rbp");
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
