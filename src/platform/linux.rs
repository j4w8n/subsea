//! Linux operation numbers. Register setup and trap instructions belong to
//! the architecture backend, not to this platform module.

pub(crate) const STDIN: u64 = 0;
pub(crate) const STDOUT: u64 = 1;

pub(crate) const SYS_READ: u64 = 0;
pub(crate) const SYS_WRITE: u64 = 1;
pub(crate) const SYS_MMAP: u64 = 9;
pub(crate) const SYS_MUNMAP: u64 = 11;
pub(crate) const SYS_EXIT: u64 = 60;
