# Limine Freestanding Example

This example is a minimal Limine-oriented x86-64 kernel built from subsea source. The Limine request start/end markers are declared with subsea `data` blocks.

Build the kernel ELF:

```sh
subsea build -t x86_64-free -T examples/limine/kernel.ld -o kernel.elf examples/limine/kernel.ss
```

Build the kernel object only:

```sh
subsea build -t x86_64-free -o kernel.o examples/limine/kernel.ss
```

Inspect the Limine request section:

```sh
readelf -S kernel.elf | grep limine_requests
```

To create a bootable Limine image, install or build Limine separately, then copy `kernel.elf` and `examples/limine/limine.conf` into the image layout expected by your Limine workflow.

Typical QEMU command after creating a bootable ISO or disk image:

```sh
qemu-system-x86_64 -M q35 -m 256M -cdrom subsea-limine.iso -serial stdio
```

This repository does not run a QEMU smoke test yet because the bootable image requires external Limine binaries and image creation tooling.
