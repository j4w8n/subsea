//! Platform-level operations used by target backends.

pub(crate) mod linux {
    pub(crate) const STDIN: u64 = 0;
    pub(crate) const STDOUT: u64 = 1;

    pub(crate) const SYS_READ: u64 = 0;
    pub(crate) const SYS_WRITE: u64 = 1;
    pub(crate) const SYS_MMAP: u64 = 9;
    pub(crate) const SYS_MUNMAP: u64 = 11;
    pub(crate) const SYS_EXIT: u64 = 60;

    pub(crate) fn emit_syscall(asm: &mut String, number: u64) {
        crate::machine::emit(&crate::machine::Instruction::Syscall { number }, asm);
    }

    pub(crate) fn emit_write_label(asm: &mut String, label: &str, len: usize) {
        crate::machine::emit(
            &crate::machine::Instruction::Move {
                dst: crate::machine::Operand::Register(String::from("rax")),
                src: crate::machine::Operand::Immediate(SYS_WRITE as i128),
            },
            asm,
        );
        asm.push_str(&format!(
            "  mov rdi, {STDOUT}\n  lea rsi, [rip + {label}]\n  mov rdx, {len}\n"
        ));
        crate::machine::emit(&crate::machine::Instruction::SyscallTrap, asm);
    }

    pub(crate) fn emit_write_registers(asm: &mut String) {
        crate::machine::emit(
            &crate::machine::Instruction::Move {
                dst: crate::machine::Operand::Register(String::from("rax")),
                src: crate::machine::Operand::Immediate(SYS_WRITE as i128),
            },
            asm,
        );
        asm.push_str(&format!("  mov rdi, {STDOUT}\n"));
        crate::machine::emit(&crate::machine::Instruction::SyscallTrap, asm);
    }

    pub(crate) fn emit_read(asm: &mut String) {
        emit_syscall(asm, SYS_READ);
    }

    pub(crate) fn emit_mmap(asm: &mut String) {
        emit_syscall(asm, SYS_MMAP);
    }

    pub(crate) fn emit_munmap(asm: &mut String) {
        emit_syscall(asm, SYS_MUNMAP);
    }
}
