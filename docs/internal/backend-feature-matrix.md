# Backend Feature Matrix
 
Status meanings:

- **Complete**: both backends meet the parity and regression criteria.
- **Implemented**: the backend has an emission path, but validation, edge
  cases, or assembler/runtime coverage is still incomplete.
- **Partial**: one or more valid source/IR forms are still unsupported or have
  different semantics.

This matrix tracks semantic feature parity between the x86-64 and AArch64
backends. Target-specific register names, instruction spelling, ABI details,
and syscall conventions are expected differences. A feature is complete only
when both backends accept the same valid source/IR forms, reject invalid forms
consistently, and have regression coverage.

| Area | x86-64 | AArch64 | Current Status |
| --- | --- | --- | --- |
| Integer arithmetic and bitwise operations | Complete | Complete | Nested expressions and memory overlap are covered |
| Power, division, and modulo | Complete | Complete | Divisor traps and edge cases are covered |
| Pair and widened arithmetic | Complete | Complete | Pair validation and assembler coverage are tested |
| Integer width conversions | Complete | Complete | Constant and cross-assembler coverage are tested |
| Integer/float casts | Complete | Complete | Trap behavior is covered by backend regression tests |
| Float arithmetic | Complete | Complete | Alias, source-form, and assembler coverage are tested |
| Float comparisons | Complete | Complete | Ordered NaN semantics and assembler coverage are tested |
| Address-of expressions | Complete | Complete | Symbol, indexed, scaled, and subtractive forms are covered |
| Indexed and scaled addressing | Complete | Complete | Scale, overlap, subtractive, and assembler coverage are tested |
| Floating-point memory loads and copies | Complete | Complete | Width, copy, and assembler coverage are tested |
| Memory moves and operand legality | Complete | Complete | Register classes, widths, and memory moves are covered |
| Push/pop | Complete | Complete | 64-bit register/memory legality and assembler coverage are tested |
| Static data sections and retention | Complete | Complete | Data sections, retention, and assembler coverage are tested |
| Stack strings and string properties | Complete | Complete | Forward constants, properties, and empty-string edges are covered |
| Runtime print formatting | Complete | Complete | Width normalization, inference, and runtime coverage are tested |
| Linux read/reserve/release | Complete | Complete | Pointer, length, destination, and runtime coverage are tested |
| Calls and indirect control flow | Complete | Complete | Stack targets, indirect branches, and assembler coverage are tested |
| Constant and binding resolution | Complete | Complete | Forward integer/string bindings and properties are covered |
| Freestanding restrictions | Complete | Complete | Covered |
| Inline assembly target checks | Complete | Complete | Covered by shared validation |

## Completion Criteria

- Each row has source-level or semantic-IR tests for both targets.
- Invalid operands fail before assembler invocation with target-appropriate
  diagnostics.
- Generated assembly is assembled by the target assembler where available.
- Runtime behavior is covered for Linux targets and QEMU-backed AArch64 tests
  where the toolchain is available.
- README claims describe partial support accurately until a row is complete.
