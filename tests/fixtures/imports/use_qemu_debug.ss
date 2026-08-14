import debug_write from "qemu_debug.ss"

main: {
  const message = "Subsea\n"
  rsi = message.ptr
  rdx = message.len
  call debug_write
  linux.exit 0
}
