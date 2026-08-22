# Freestanding Example

This example builds a tiny x86-64 freestanding halt loop.

Run these commands from the repository root.

Build an object file:

```sh
subsea build -t x86-free -o kernel.o examples/freestanding/kernel.ss
```

Link an ELF with a linker script:

```sh
subsea build -t x86-free -T examples/freestanding/kernel.ld -o kernel.elf examples/freestanding/kernel.ss
```

Build a raw binary from the linked ELF:

```sh
subsea build -t x86-free -T examples/freestanding/kernel.ld --format binary -o kernel.bin examples/freestanding/kernel.ss
```

The raw binary is not bootable by itself. It has no boot sector, firmware header, or bootloader protocol metadata yet.
