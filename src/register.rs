//! Register spelling recognized by the target-neutral lexer.
//!
//! This intentionally accepts the union of supported register syntaxes. It
//! must not be used for target legality checks; those go through `Target`.

pub(crate) fn is_lexical_register(name: &str) -> bool {
    crate::backend::x86_64::is_register(name) || crate::backend::aarch64::is_register(name)
}

pub(crate) fn is_lexical_vector_register(name: &str) -> bool {
    crate::backend::x86_64::is_vector(name) || crate::backend::aarch64::is_vector(name)
}
