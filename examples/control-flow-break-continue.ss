main: {
  r8 = 0

.loop:
  r8 = r8 + 1
  jmp .done if r8 u> 10  // break
  jmp .loop if r8 == 5   // continue
  linux.print r8
  linux.print "\n"
  jmp .loop

.done:
  linux.exit 0
}
