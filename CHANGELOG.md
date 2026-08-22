# Changelog

All notable user-visible changes to Subsea are documented here.

## [0.1.0] - 2026-08-21

First public release.

### Added

- Stable x86-64 Linux compilation through the `x86` target.
- A prebuilt static musl x86-64 Linux compiler binary.
- Register, memory, integer, scalar floating-point, control-flow, function, import, stack, layout, static-data, and Linux runtime features described in the language reference.
- Assembly emission with optional source annotations.
- Experimental `x86-free`, `aarch`, and `aarch-free` targets.
- Experimental freestanding object, linker-script ELF, and raw binary output.

### Known Limitations

- Runtime floating-point printing is not supported.
- Raw freestanding binaries are not bootable without platform-specific boot support.
- Experimental targets do not carry the stable target's compatibility commitment.

[0.1.0]: https://github.com/j4crev/subsea/releases/tag/v0.1.0
