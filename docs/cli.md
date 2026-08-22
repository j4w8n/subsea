# CLI Reference

Use `subsea help`, `subsea help run`, `subsea help build`, or `subsea help emit-asm` for the installed version's authoritative usage.

## Commands

```sh
subsea run main.ss
subsea build main.ss
subsea emit-asm main.ss
subsea emit-asm --annotate main.ss
```

- `run` compiles, links, and executes a Linux program. It exits with the program's exit code.
- `build` writes an executable for Linux targets. For freestanding targets it writes an object by default, or a linked ELF when given `-T`.
- `emit-asm` writes target assembly to stdout. `--annotate` adds source locations, source statements, imported-source locations, and generated-region comments.

Intermediate assembly and objects use unique directories under `target/subsea/build-*`. The default Linux executable is `target/subsea/main`; the default freestanding object is `target/subsea/main.o`.

## Run Options

`run` accepts only `x86` and `aarch`. It runs directly unless `--runner <program>` is supplied. Program arguments must follow `--`:

```sh
subsea run -t aarch --runner qemu-aarch64 main.ss -- argument-one --flag
```

## Build Options

```text
-t, --target <TARGET>       Select x86, x86-free, aarch, or aarch-free
-o <PATH>                   Set the output path
    --timings               Print phase timings
    --entry <SYMBOL>        Set the freestanding linker-visible entry symbol
-T, --linker-script <PATH>  Link a freestanding object with a linker script
    --link-input <PATH>     Add an object to the freestanding link; repeatable
    --format <FORMAT>       Select elf (default) or binary
    --linker <PROGRAM>      Select the freestanding linker
```

Linux targets reject freestanding-only options. `--link-input` and `--format binary` require a freestanding target and `-T`. Binary output links an ELF and then invokes `objcopy -O binary`.

```sh
subsea build -o my_util main.ss
subsea build -t x86-free -o kernel.o kernel.ss
subsea build -t x86-free -T kernel.ld -o kernel.elf kernel.ss
subsea build -t x86-free -T kernel.ld --format binary -o kernel.bin kernel.ss
subsea build -t x86-free --linker ld.lld -T kernel.ld -o kernel.elf kernel.ss
subsea build -t x86-free -T kernel.ld --link-input boot.o --link-input tables.o -o kernel.elf kernel.ss
subsea build --timings main.ss
```

For freestanding builds, `--entry` changes the linker-visible symbol emitted for source-level `main`; the source function remains named `main`.

See [Targets and Toolchains](targets.md) for target status and external tools.
