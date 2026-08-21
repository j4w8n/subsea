pub(crate) mod asm;
pub(crate) mod codegen;
mod registers;

pub(crate) use codegen::emit_for_target_with_entry;
pub(crate) use registers::is_register;
pub(crate) use registers::is_vector;
