main: {
  stack buffer:u8(16)
  [buffer] = "Hello"

  stack message:str = slice(&buffer, 5)
  linux.print message
  linux.print "\n"

  buffer[0]:u8 = 74
  linux.print message
  linux.print "\n"

  linux.exit 0
}
