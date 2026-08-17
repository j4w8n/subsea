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
        asm.push_str(&format!("  mov rax, {number}\n  syscall\n"));
    }

    pub(crate) fn emit_write_label(asm: &mut String, label: &str, len: usize) {
        asm.push_str(&format!(
            "  mov rax, {SYS_WRITE}\n  mov rdi, {STDOUT}\n  lea rsi, [rip + {label}]\n  mov rdx, {len}\n  syscall\n"
        ));
    }

    pub(crate) fn emit_write_registers(asm: &mut String) {
        asm.push_str(&format!(
            "  mov rax, {SYS_WRITE}\n  mov rdi, {STDOUT}\n  syscall\n"
        ));
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
