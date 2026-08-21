main: {
  r8 = 0

.loop:
  jmp .done if r8 u>= 5
  linux.print r8
  linux.print "\n"
  r8 = r8 + 1
  jmp .loop

.done:
  linux.exit 0
}
