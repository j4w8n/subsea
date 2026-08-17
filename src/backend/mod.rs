pub mod aarch64;
pub(crate) mod x86_64;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Architecture {
    X86_64,
    AArch64,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Environment {
    Linux,
    Freestanding,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum RuntimeOperation {
    Exit,
    Read,
    Write,
    Reserve,
    Release,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FramePointerPolicy {
    Required,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum EntryConvention {
    ProcessEntry,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct TargetSpec {
    pub architecture: Architecture,
    pub environment: Environment,
    pub pointer_width: u8,
    pub pointer_alignment: usize,
    pub linker_emulation: &'static str,
    pub assembler: &'static str,
    pub linker: &'static str,
    pub objcopy: &'static str,
    pub stack_alignment: usize,
    pub stack_pointer: &'static str,
    pub frame_pointer: &'static str,
    pub frame_pointer_policy: FramePointerPolicy,
    pub entry_convention: EntryConvention,
    pub runtime_call_convention: &'static str,
    pub integer_argument_registers: &'static [&'static str],
    pub integer_return_register: &'static str,
    pub float_argument_registers: &'static [&'static str],
    pub float_return_register: &'static str,
    pub caller_saved_registers: &'static [&'static str],
    pub callee_saved_registers: &'static [&'static str],
    pub runtime_operations: &'static [RuntimeOperation],
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Target {
    X86_64,
    X86_64Free,
    AArch64Linux,
}

impl Target {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "x86" => Ok(Self::X86_64),
            "x86-free" => Ok(Self::X86_64Free),
            "aarch" => Ok(Self::AArch64Linux),
            _ => Err(format!(
                "Unknown target {name:?}; expected x86, x86-free, or aarch"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::X86_64 => "x86",
            Self::X86_64Free => "x86-free",
            Self::AArch64Linux => "aarch",
        }
    }

    pub fn spec(self) -> TargetSpec {
        if self == Self::AArch64Linux {
            return TargetSpec {
                architecture: Architecture::AArch64,
                environment: Environment::Linux,
                pointer_width: 64,
                pointer_alignment: 8,
                linker_emulation: "aarch64elf",
                assembler: "aarch64-linux-gnu-as",
                linker: "aarch64-linux-gnu-ld",
                objcopy: "aarch64-linux-gnu-objcopy",
                stack_alignment: 16,
                stack_pointer: "sp",
                frame_pointer: "x29",
                frame_pointer_policy: FramePointerPolicy::Required,
                entry_convention: EntryConvention::ProcessEntry,
                runtime_call_convention: "aapcs64",
                integer_argument_registers: &["x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7"],
                integer_return_register: "x0",
                float_argument_registers: &["v0", "v1", "v2", "v3", "v4", "v5", "v6", "v7"],
                float_return_register: "v0",
                caller_saved_registers: &[
                    "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10", "x11",
                    "x12", "x13", "x14", "x15", "x16", "x17", "x18", "v0", "v1", "v2", "v3", "v4",
                    "v5", "v6", "v7",
                ],
                callee_saved_registers: &[
                    "x19", "x20", "x21", "x22", "x23", "x24", "x25", "x26", "x27", "x28",
                ],
                runtime_operations: &[
                    RuntimeOperation::Exit,
                    RuntimeOperation::Read,
                    RuntimeOperation::Write,
                    RuntimeOperation::Reserve,
                    RuntimeOperation::Release,
                ],
            };
        }

        TargetSpec {
            architecture: Architecture::X86_64,
            environment: match self {
                Self::X86_64 => Environment::Linux,
                Self::X86_64Free => Environment::Freestanding,
                Self::AArch64Linux => Environment::Linux,
            },
            pointer_width: 64,
            pointer_alignment: 8,
            linker_emulation: "elf_x86_64",
            assembler: "as",
            linker: "ld",
            objcopy: "objcopy",
            stack_alignment: 16,
            stack_pointer: "rsp",
            frame_pointer: "rbp",
            frame_pointer_policy: FramePointerPolicy::Required,
            entry_convention: EntryConvention::ProcessEntry,
            runtime_call_convention: "sysv_amd64",
            integer_argument_registers: &["rdi", "rsi", "rdx", "rcx", "r8", "r9"],
            integer_return_register: "rax",
            float_argument_registers: &[
                "xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5", "xmm6", "xmm7",
            ],
            float_return_register: "xmm0",
            caller_saved_registers: &[
                "rax", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11", "xmm0", "xmm1",
                "xmm2", "xmm3", "xmm4", "xmm5", "xmm6", "xmm7", "xmm8", "xmm9", "xmm10", "xmm11",
                "xmm12", "xmm13", "xmm14", "xmm15",
            ],
            callee_saved_registers: &["rbx", "rbp", "r12", "r13", "r14", "r15"],
            runtime_operations: match self {
                Self::X86_64 => &[
                    RuntimeOperation::Exit,
                    RuntimeOperation::Read,
                    RuntimeOperation::Write,
                    RuntimeOperation::Reserve,
                    RuntimeOperation::Release,
                ],
                Self::X86_64Free => &[],
                Self::AArch64Linux => &[
                    RuntimeOperation::Exit,
                    RuntimeOperation::Read,
                    RuntimeOperation::Write,
                    RuntimeOperation::Reserve,
                    RuntimeOperation::Release,
                ],
            },
        }
    }

    pub fn is_freestanding(self) -> bool {
        matches!(self.spec().environment, Environment::Freestanding)
    }

    pub fn supports_runtime(self, operation: RuntimeOperation) -> bool {
        self.spec().runtime_operations.contains(&operation)
    }

    pub(crate) fn is_register(self, name: &str) -> bool {
        match self.spec().architecture {
            Architecture::X86_64 => x86_64::is_register(name),
            Architecture::AArch64 => aarch64::is_register(name),
        }
    }
}
