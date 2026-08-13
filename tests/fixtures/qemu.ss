main: {
  al = 83  // S
  x86 "out 0xe9, al"

.hang:
  x86 "hlt"
  jmp .hang
}