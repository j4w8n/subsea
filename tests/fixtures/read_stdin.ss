mem buf:u8(1024)

main: {
  linux.read(stdin, &buf, 1024)
  stack input:str = slice(&buf, rax)
  linux.print input
  linux.exit 0
}
