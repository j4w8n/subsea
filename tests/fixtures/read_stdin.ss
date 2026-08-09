mem buf:u8(1024)

main: {
  read stdin, &buf, 1024
  stack input:str = slice &buf, rax
  print input
  exit 0
}
