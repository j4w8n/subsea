# Examples

## Subsea And x86-64 Assembly

The [side-by-side examples](../examples/subsea-vs-x86/README.md) compare subsea programs with x86-64 assembly equivalents.

## Control Flow

The top-level `examples/control-flow-*.ss` programs demonstrate while, do-while, for, if/else, guard clauses, break/continue, state machines, and array iteration using local labels and jumps.

## Experimental Freestanding Examples

- [Freestanding x86-64](../examples/freestanding/README.md) builds an object, linked ELF, or raw binary.
- [Limine kernel](../examples/limine/README.md) demonstrates request metadata, ISO staging, and a manual QEMU debug-console smoke test.

Contributors can build and inspect the documented examples with `cargo test --test examples`; external Limine assets, ISO creation, and QEMU remain manual.
