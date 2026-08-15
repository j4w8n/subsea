main: {
  rax = -1
  rdx = 0
  rbx = 1
  rcx = 0
  rdx:rax = rdx:rax + rcx:rbx

  rbx = 1
  rcx = 0
  rdx:rax = rdx:rax - rcx:rbx

  linux.exit 0
}
