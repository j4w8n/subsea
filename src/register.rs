use crate::analysis::Width;

pub(crate) fn width(name: &str) -> Option<Width> {
    crate::backend::x86_64::width(name)
}

pub fn is_register(name: &str) -> bool {
    crate::backend::x86_64::is_register(name)
}

pub(crate) fn is_xmm(name: &str) -> bool {
    crate::backend::x86_64::is_xmm(name)
}
