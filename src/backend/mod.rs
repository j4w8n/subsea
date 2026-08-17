pub(crate) mod x86_64;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Architecture {
    X86_64,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Environment {
    Linux,
    Freestanding,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct TargetSpec {
    pub architecture: Architecture,
    pub environment: Environment,
    pub pointer_width: u8,
    pub linker_emulation: &'static str,
    pub stack_alignment: usize,
    pub stack_pointer: &'static str,
    pub frame_pointer: &'static str,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Target {
    X86_64,
    X86_64Free,
}

impl Target {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "x86_64" => Ok(Self::X86_64),
            "x86_64-free" => Ok(Self::X86_64Free),
            _ => Err(format!(
                "Unknown target {name:?}; expected x86_64 or x86_64-free"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::X86_64Free => "x86_64-free",
        }
    }

    pub fn spec(self) -> TargetSpec {
        TargetSpec {
            architecture: Architecture::X86_64,
            environment: match self {
                Self::X86_64 => Environment::Linux,
                Self::X86_64Free => Environment::Freestanding,
            },
            pointer_width: 64,
            linker_emulation: "elf_x86_64",
            stack_alignment: 16,
            stack_pointer: "rsp",
            frame_pointer: "rbp",
        }
    }

    pub fn is_freestanding(self) -> bool {
        matches!(self.spec().environment, Environment::Freestanding)
    }
}
