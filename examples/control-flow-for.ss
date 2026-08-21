main: {
  r8 = 0   // i
  r9 = 10  // limit

.loop:
  jmp .done if r8 u>= r9
  linux.print r8
  linux.print "\n"
  r8 = r8 + 1
  jmp .loop

.done:
  linux.exit 0
}
