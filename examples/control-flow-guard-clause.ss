main: {
  rax = 0
  jmp .fail if rax == 0

  linux.print "ok\n"
  linux.exit 0

.fail:
  linux.print "fail\n"
  linux.exit 1
}
