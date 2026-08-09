mem buf:u8(1)

main: {
  r10 = 7
  stack input:str = slice &buf, 0
  print r10
  print "\n"
  exit 0
}
