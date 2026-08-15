mem buf:u8(16)

main: {
  const hi = "Hi\n"
  [buf] = hi
  buf[3] = "Bye\n"

  stack first:str = slice(&buf, 3)
  linux.print first

  stack second:str = slice(&buf[3], 4)
  linux.print second

  linux.exit 0
}
