# Limine Freestanding Example

This example is a minimal Limine-oriented x86-64 kernel built from subsea source. The Limine request start/end markers are declared with subsea `data` blocks.

The `iso_root` directory should contain the Limine boot files, `limine.conf`, and the built kernel at `iso_root/boot/kernel.elf`.

Build the kernel ELF into the ISO staging tree:

```sh
subsea build -t x86_64-free -T kernel.ld -o iso_root/boot/kernel.elf kernel.ss
```

Build the kernel object only:

```sh
subsea build -t x86_64-free -o kernel.o examples/limine/kernel.ss
```

Inspect the Limine request section:

```sh
readelf -S iso_root/boot/kernel.elf | grep limine_requests
```

Build the ISO:

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
  iso_root \
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

Bytes written by `x86 "out 0xe9, al"` appear in the terminal where QEMU is running. The display window may stay blank because this example does not write to the framebuffer.

This example is a manual QEMU smoke test. It is not run by `cargo test` because it requires external Limine binaries, ISO creation tools, and QEMU.
