export debug_write: {
.loop:
  jmp .done if rdx == 0

  call debug_write_byte
  rsi = rsi + 1
  rdx = rdx - 1
  jmp .loop

.done:
  ret
}

debug_write_byte: {
  al = [rsi]:u8
  x86 "out 0xe9, al"
  ret
}
