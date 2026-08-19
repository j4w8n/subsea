pub(crate) mod asm;
pub(crate) mod codegen;
mod registers;

pub use codegen::{emit, emit_for_target, emit_for_target_with_entry};
pub use registers::is_register;
pub(crate) use registers::is_vector;
