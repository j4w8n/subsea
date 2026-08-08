use crate::codegen::Width;

pub(crate) fn width(name: &str) -> Option<Width> {
    Some(match name {
        "rax" | "rbx" | "rcx" | "rdx" | "rdi" | "rsi" | "rbp" | "rsp" | "r8" | "r9" | "r10"
        | "r11" | "r12" | "r13" | "r14" | "r15" => Width::Bits64,
        "eax" | "ebx" | "ecx" | "edx" | "edi" | "esi" | "ebp" | "esp" | "r8d" | "r9d" | "r10d"
        | "r11d" | "r12d" | "r13d" | "r14d" | "r15d" => Width::Bits32,
        "ax" | "bx" | "cx" | "dx" | "di" | "si" | "bp" | "sp" | "r8w" | "r9w" | "r10w" | "r11w"
        | "r12w" | "r13w" | "r14w" | "r15w" => Width::Bits16,
        "al" | "bl" | "cl" | "dl" | "ah" | "bh" | "ch" | "dh" | "dil" | "sil" | "bpl" | "spl"
        | "r8b" | "r9b" | "r10b" | "r11b" | "r12b" | "r13b" | "r14b" | "r15b" => Width::Bits8,
        _ => return None,
    })
}

pub fn is_register(name: &str) -> bool {
    width(name).is_some()
}
