main: {
  r8 = 0

.loop:
  linux.print r8
  linux.print "\n"
  r8 = r8 + 1
  jmp .loop if r8 u< 5

  linux.exit 0
}
