main: {
  rax = linux.reserve(4096)
  jmp .error if rax i< 0

  [rax]:u8 = 72
  [rax + 1]:u8 = 105
  [rax + 2]:u8 = 10

  stack message:str = slice(rax, 3)
  linux.print message

  linux.release(rax, 4096)
  jmp .error if rax i< 0

  linux.exit 0

.error:
  linux.exit 1
}
