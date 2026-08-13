main: {
  al = 83  // S
  out 0xe9, al

.hang:
  hlt
  jmp .hang
}