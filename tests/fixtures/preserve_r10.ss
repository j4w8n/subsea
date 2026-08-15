mem buf:u8(1)

main: {
  r10 = 7
  stack input:str = slice(&buf, 0)
  linux.print r10
  linux.print "\n"
  linux.exit 0
}
