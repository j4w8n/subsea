mem buf:u8(16)

main: {
  [buf] = "Hi\n"
  buf[3] = "Bye\n"

  stack first:str = slice(&buf, 3)
  linux.print first

  rsi = &buf[3]
  stack second:str = slice(rsi, 4)
  linux.print second

  linux.exit 0
}
