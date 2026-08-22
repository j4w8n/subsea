# Targets And Toolchains

Release stability and implementation completion are separate. A feature may be implemented and tested on an experimental backend without making that target stable for v0.1.

| Target | Output environment | v0.1 status |
| --- | --- | --- |
| `x86` | x86-64 Linux userland | Stable, default |
| `x86-free` | x86-64 freestanding | Experimental |
| `aarch` | AArch64 Linux userland | Experimental |
| `aarch-free` | AArch64 freestanding | Experimental |

## Stable x86

The supported host/target combination is x86-64 Linux compiling `x86`. The prebuilt compiler is a static musl binary; users need only `subsea` plus GNU `as` and `ld`. Linux helpers such as `linux.print`, `linux.read`, `linux.reserve`, `linux.release`, and `linux.exit` are available.

## AArch64 Linux

Select experimental AArch64 Linux output with `--target aarch` or `-t aarch`. Building requires an appropriate AArch64 GNU assembler and linker. Cross-running can use an explicit runner:

```sh
subsea emit-asm -t aarch main.ss
subsea build -t aarch main.ss
subsea run -t aarch --runner qemu-aarch64 main.ss
```

## Freestanding

`x86-free` and `aarch-free` reject Linux helpers because a freestanding environment does not provide Linux process services. A build produces an object unless a linker script is supplied:

```sh
subsea build -t x86-free -o kernel.o kernel.ss
subsea build -t x86-free -T kernel.ld -o kernel.elf kernel.ss
subsea build -t aarch-free -o kernel.o kernel.ss
subsea build -t aarch-free -T kernel.ld -o kernel.elf kernel.ss
```

Linker-script builds pass the target linker emulation, such as `elf_x86_64` or `aarch64elf`. Use `--linker` to select a cross-linker or `ld.lld`.

Raw binary output is experimental and requires `objcopy`:

```sh
subsea build -t x86-free -T kernel.ld --format binary -o kernel.bin kernel.ss
```

A raw binary is not bootable by itself. It still needs the appropriate boot sector, firmware header, boot protocol metadata, or bootloader. See the [freestanding](../examples/freestanding/README.md) and [Limine](../examples/limine/README.md) examples.
