# Limine Freestanding Example

This example is a minimal Limine-oriented x86-64 kernel built from subsea source. The Limine request start/end markers are declared with subsea `data` blocks.

The `examples/limine/iso_root` directory should contain the external Limine boot files, `limine.conf`, and the built kernel at `examples/limine/iso_root/boot/kernel.elf`. Run the commands below from the repository root.

Build the kernel ELF into the ISO staging tree:

```sh
subsea build -t x86-free -T examples/limine/kernel.ld -o examples/limine/iso_root/boot/kernel.elf examples/limine/kernel.ss
```

Build the kernel object only:

```sh
subsea build -t x86-free -o kernel.o examples/limine/kernel.ss
```

Inspect the Limine request section:

```sh
readelf -S examples/limine/iso_root/boot/kernel.elf | grep limine_requests
```

Build the ISO from the staging tree:

```sh
xorriso -as mkisofs -R -r -J \
  -b boot/limine-bios-cd.bin \
  -no-emul-boot \
  -boot-load-size 4 \
  -boot-info-table \
  -hfsplus \
  -apm-block-size 2048 \
  --efi-boot boot/limine-uefi-cd.bin \
  -efi-boot-part \
  --efi-boot-image \
  --protective-msdos-label \
  examples/limine/iso_root \
  -o subsea.iso
```

Install Limine BIOS stages:

```sh
limine bios-install subsea.iso
```

Run with QEMU debugcon on port `0xe9`:

```sh
qemu-system-x86_64 -M q35 -m 256M -cdrom subsea.iso -debugcon stdio -global isa-debugcon.iobase=0xe9
```

Bytes written by `asm.x86 "out 0xe9, al"` appear in the terminal where QEMU is running. The display window may stay blank because this example does not write to the framebuffer.

`cargo test --test examples` builds the kernel ELF and inspects its request section without external Limine assets. ISO creation and this QEMU smoke test remain manual because they require Limine binaries, ISO creation tools, and QEMU.
