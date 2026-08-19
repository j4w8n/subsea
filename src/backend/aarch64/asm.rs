//! AArch64 assembly syntax helpers.
//!
//! Instruction selection and runtime policy remain in `codegen`; this module
//! only centralizes common assembler spelling.

use std::fmt::{Display, Write};

pub(crate) fn label(asm: &mut String, name: &str) {
    asm.push_str(name);
    asm.push_str(":\n");
}

pub(crate) fn instruction(asm: &mut String, text: impl Display) {
    let _ = writeln!(asm, "  {text}");
}

pub(crate) fn mov(asm: &mut String, dst: impl Display, src: impl Display) {
    instruction(asm, format_args!("mov {dst}, {src}"));
}

pub(crate) fn load(asm: &mut String, opcode: &str, dst: impl Display, address: impl Display) {
    instruction(asm, format_args!("{opcode} {dst}, {address}"));
}

pub(crate) fn store(asm: &mut String, opcode: &str, src: impl Display, address: impl Display) {
    instruction(asm, format_args!("{opcode} {src}, {address}"));
}

pub(crate) fn branch(asm: &mut String, opcode: &str, target: impl Display) {
    instruction(asm, format_args!("{opcode} {target}"));
}

pub(crate) fn call(asm: &mut String, target: impl Display) {
    instruction(asm, format_args!("bl {target}"));
}

pub(crate) fn call_register(asm: &mut String, register: &str) {
    instruction(asm, format_args!("blr {register}"));
}

pub(crate) fn ret(asm: &mut String) {
    instruction(asm, "ret");
}

pub(crate) fn svc(asm: &mut String) {
    instruction(asm, "svc #0");
}

pub(crate) fn section(asm: &mut String, name: &str) {
    asm.push_str(".section .");
    asm.push_str(name);
    asm.push('\n');
}

pub(crate) fn text(asm: &mut String) {
    top_level_directive(asm, ".text");
}

pub(crate) fn directive(asm: &mut String, text: impl Display) {
    let _ = writeln!(asm, "  {text}");
}

pub(crate) fn top_level_directive(asm: &mut String, text: impl Display) {
    let _ = writeln!(asm, "{text}");
}

pub(crate) fn global(asm: &mut String, name: &str) {
    asm.push_str(".global ");
    asm.push_str(name);
    asm.push('\n');
}
