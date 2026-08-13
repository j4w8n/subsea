main: {
  rax = 5

  rbx = rax & 4 != 0
  linux.print rbx
  linux.print "\n"

  rax = 5
  rbx = 0
  rbx = 9 if rax & 4 != 0
  linux.print rbx
  linux.print "\n"

  rax = 5
  rcx = rax i> 10
  linux.print rcx
  linux.print "\n"

  linux.exit 0
}
