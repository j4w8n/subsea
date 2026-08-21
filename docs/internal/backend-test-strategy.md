# Backend Test Strategy

Backend coverage is split by the boundary being tested. Test count is not
expected to match between architectures; coverage of equivalent risk areas is
the goal.

## Public Contract Tests

Public AST-level behavior belongs in `tests/codegen.rs`:

- Build programs through the public AST types and codegen entry points.
- Verify generated assembly, diagnostics, and target-independent behavior.
- Assemble generated output when the target assembler is available.
- Keep these tests external so they cannot depend on private semantic IR or
  backend implementation names.

These tests are the source-level contract for every backend. Architecture-
specific assembly spelling is acceptable when it is the behavior being
validated, but the input should remain a public AST program.

## Backend Lowering Tests

Private semantic-IR and lowering tests belong beside the backend:

- `src/backend/aarch64/codegen_tests.rs`
- `src/backend/x86_64/codegen_tests.rs`

Use this layer for behavior that cannot be expressed through the public AST
without losing the invariant under test:

- Scratch-register allocation and conflict handling.
- Fixed architectural result pairs.
- Backend-only operand normalization.
- Instruction selection for semantic-IR variants.
- Internal diagnostics that must be raised before assembly.

These suites do not need identical sizes. Each backend should cover the same
categories of lowering risk, while tests remain specific to the instructions,
registers, and ABI rules of that architecture.

## Integration Tests

CLI, assembler, linker, runtime, and QEMU behavior belongs in
`tests/integration.rs` or dedicated fixtures:

- Exercise complete source programs.
- Verify target selection and diagnostics.
- Run the native assembler/linker where available.
- Run Linux and QEMU behavior where the toolchain exists.

## Adding A Backend

For a new backend, add coverage in this order:

1. Public AST/source-level tests in `tests/codegen.rs`.
2. Backend-private lowering tests beside its `codegen.rs`.
3. Assembler and runtime tests in `tests/integration.rs`.

Before marking a feature complete, verify valid forms, invalid-form
diagnostics, assembler coverage, and runtime behavior against the feature
matrix. Do not move public contract tests into private backend modules merely
to equalize test counts.
