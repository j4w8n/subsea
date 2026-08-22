# x86 Intel Assembly vs Subsea

These examples compare readable Subsea source with equivalent x86-64 Linux programs written in Intel assembly syntax.

The goal is not only fewer lines of code. The goal is to show where Subsea keeps the same low-level model, including registers, memory, labels, syscalls, functions, and indirect control flow, while making intent easier to read.

## Files

| Example | Assembly | Subsea | Focus |
| --- | --- | --- | --- |
| Hello | `01_hello.asm` | `01_hello.ss` | Linux `write` and `exit` |
| Arithmetic | `02_arithmetic.asm` | `02_arithmetic.ss` | registers, expressions, comparisons, branches |
| Array sum | `03_array_sum.asm` | `03_array_sum.ss` | memory, indexed loads, loops, formatted output |
| Function comparison | `04_function_compare.asm` | `04_function_compare.ss` | functions, explicit result registers, basic addition |
| Dispatch table | `05_dispatch_table.asm` | `05_dispatch_table.ss` | functions, function pointers, indirect calls |
| Layout | `06_layout.asm` | `06_layout.ss` | padded memory layouts, symbolic offsets, alignment |

## Try The Subsea Versions

From the repository root:

```bash
subsea run examples/subsea-vs-x86/01_hello.ss
subsea run examples/subsea-vs-x86/02_arithmetic.ss
subsea run examples/subsea-vs-x86/03_array_sum.ss
subsea run examples/subsea-vs-x86/04_function_compare.ss
subsea run examples/subsea-vs-x86/05_dispatch_table.ss
subsea run examples/subsea-vs-x86/06_layout.ss
```

If you are developing Subsea from this checkout, use Cargo:

```bash
cargo run -- run examples/subsea-vs-x86/01_hello.ss
```

## Reading Notes

- The `.asm` files use GNU assembler directives with `.intel_syntax noprefix`, so operands are written in Intel order: `mov destination, source`.
- The `.ss` files intentionally keep registers visible. Subsea is still low-level; it just replaces common instruction sequences with direct assignment, typed memory, `linux.print`, `linux.exit`, and explicit signed/unsigned operators.
- Assembly needs manual string lengths, syscall register setup, and formatting helpers. Subsea can still use raw syscalls, but these examples use its higher-level Linux conveniences where they improve readability.
- Subsea comparisons require signedness for ordered integer checks, such as `i>` or `u>=`, making intent explicit where assembly uses condition-code mnemonics like `jg` or `jae`.
- Functions can choose an explicit result register. In `04_function_compare`, `add` stores its result in `rcx`, and the caller uses that register after `call`.
