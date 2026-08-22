main: {
  x0 = 1
  asm.aarch64 "nop"
.halt:
  jmp .halt
}
