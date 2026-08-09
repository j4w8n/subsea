main: {
  stack message:str = "props\n"

  rax = 1
  rdi = 1
  rsi = message.ptr
  rdx = message.len
  syscall

  print message.len
  print "\n"

  exit 0
}
