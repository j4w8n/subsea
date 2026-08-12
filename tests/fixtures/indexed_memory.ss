mem values:u64(3)
mem bytes:u8(8)

main: {
  values[0] = 10
  values[8] = 20
  values[16] = 30

  r8 = 0
  r9 = 3

.loop:
  jmp .done if r8 u>= r9
  rax = values[r8 * 8]
  print rax
  print "\n"
  r8 = r8 + 1
  jmp .loop

.done:
  bytes[0] = 72
  bytes[1] = 105
  bytes[2] = 33
  bytes[3] = 10

  rsi = &bytes[1]
  stack message:str = slice rsi, 3
  print message

  exit 0
}
