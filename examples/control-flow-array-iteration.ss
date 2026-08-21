mem values:u64(4)

main: {
  values[0] = 10
  values[8] = 20
  values[16] = 30
  values[24] = 40

  r8 = 0
  r9 = 4

.loop:
  jmp .done if r8 u>= r9
  rax = values[r8 * 8]
  linux.print rax
  linux.print "\n"
  r8 = r8 + 1
  jmp .loop

.done:
  linux.exit 0
}
