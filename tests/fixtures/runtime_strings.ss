mem buf:u8(8)

main: {
  stack literal:str = "literal\n"

  [buf]:u8 = 72
  [buf + 1]:u8 = 105
  stack input:str = slice &buf, 2

  print literal
  print input
  print "\n"

  exit 0
}
