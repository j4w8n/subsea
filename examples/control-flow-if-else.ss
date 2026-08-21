main: {
  rax = 3

  jmp .nonzero if rax != 0
  linux.print "zero\n"
  jmp .done

.nonzero:
  linux.print "non-zero\n"

.done:
  linux.exit 0
}
