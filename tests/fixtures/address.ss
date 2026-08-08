main: {
  rsp = rsp - 8

  const char = 65
  [rsp]:u8 = char

  rax = 1    // write syscall
  rdi = 1    // stdout
  rsi = rsp  // buffer address
  rdx = 1    // byte count
  syscall

  exit 0
}
