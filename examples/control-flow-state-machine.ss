main: {
  rax = 0
  jmp .state_start

.state_start:
  linux.print "start\n"
  rax = 1
  jmp .state_done if rax == 1
  jmp .state_error

.state_done:
  linux.print "done\n"
  linux.exit 0

.state_error:
  linux.print "error\n"
  linux.exit 1
}
