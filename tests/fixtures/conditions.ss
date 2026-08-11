main: {
  rax = 5

  rbx = rax & 4 != 0
  print rbx
  print "\n"

  rax = 5
  rbx = 0
  rbx = 9 if rax & 4 != 0
  print rbx
  print "\n"

  rax = 5
  rcx = rax i> 10
  print rcx
  print "\n"

  exit 0
}
