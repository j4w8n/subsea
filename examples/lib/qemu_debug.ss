export debug_write: {
.loop:
  jmp .done if rdx == 0

  al = [rsi]:u8
  asm.x86 "out 0xe9, al"

  rsi = rsi + 1
  rdx = rdx - 1
  jmp .loop

.done:
  ret
}
