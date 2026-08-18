main: {
  al = 83  // S
  asm.x86 "out 0xe9, al"

.hang:
  asm.x86 "hlt"
  jmp .hang
}
