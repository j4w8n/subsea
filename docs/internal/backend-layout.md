# Backend Layout

This document describes the internal boundary for architecture backends. It is
intended for contributors and code-generation agents adding or changing a
backend.

## Directory Shape

Each supported architecture should use the same four module roles:

```text
src/backend/<architecture>/
├── mod.rs
├── codegen.rs
├── asm.rs
└── registers.rs
```

The files have the same names and responsibilities for every architecture.
Their implementations remain architecture-specific.

## `mod.rs`

`mod.rs` is the architecture module boundary. It should:

- Declare `codegen`, `asm`, and `registers`.
- Re-export the backend's public entry points.
- Re-export register predicates needed by shared target validation.
- Contain only small architecture-wide helpers that are shared by `codegen`
  and `asm`.

It should not contain the main semantic-IR-to-assembly lowering loop.

## `codegen.rs`

`codegen.rs` owns architecture-specific lowering decisions. It should:

- Consume the shared semantic IR from `crate::ir`.
- Validate target-specific operations that cannot be handled by shared code.
- Select instructions, registers, calling conventions, stack layouts, and
  runtime sequences.
- Call `asm` helpers to produce assembly text.
- Convert backend failures into `BackendError` values with source context when
  available.

The x86-64 backend also retains AST-based public wrapper functions for
compatibility. Those wrappers lower the AST first and then call the same IR
entry point; the backend's primary implementation must remain the IR path.

This is the policy and lowering layer. It decides *what* needs to be emitted,
not the spelling of every assembly instruction.

## `asm.rs`

`asm.rs` owns the target assembler syntax. It should:

- Format registers, immediates, symbols, and memory addresses.
- Emit small target-specific instructions or instruction sequences.
- Own section directives and other assembler syntax when that syntax is
  target-specific.
- Keep formatting consistent across all codegen paths.

It should not inspect the semantic IR or decide whether a source operation is
legal for a target. Runtime policy and instruction selection belong in
`codegen.rs`.

An `asm` module may use a small structured instruction type when that type
meaningfully prevents duplicated formatting. It should not become a second
compiler IR unless the project later needs machine-level analysis,
optimization, or validation.

## `registers.rs`

`registers.rs` owns the architecture's register vocabulary and predicates:

- Register recognition.
- Register families and widths.
- Vector or floating-point register classification.
- Other purely register-level facts.

Calling convention policy and stack-frame decisions belong in `codegen.rs` or
the shared `TargetSpec`, not in this file.

## Shared Layers

The architecture modules sit below these shared layers:

```text
source AST
  -> shared validation and lowering
  -> backend/<architecture>/codegen.rs
  -> backend/<architecture>/asm.rs
  -> assembly text
  -> driver assembler/linker
```

`src/codegen.rs` dispatches between architectures and owns shared diagnostics.
`src/backend/mod.rs` owns target descriptions and backend-wide contracts.
`src/platform/` owns platform metadata such as syscall numbers; it should not
own architecture-specific register setup or trap instructions.

## Compatibility

The x86-64 backend historically exposed a `backend::x86_64_machine` module.
When replacing or relocating its formatter, preserve that path as a
compatibility re-export until intentionally making a public API change.
