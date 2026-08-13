main: {
  stack message:str = "props\n"

  rax = 1
  rdi = 1
  rsi = message.ptr
  rdx = message.len
  linux.syscall

  linux.print message.len
  linux.print "\n"

  linux.exit 0
}
