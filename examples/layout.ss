// A Header has a u8, a naturally aligned u32, and a pointer-sized field.
// The layout calculates the padding, total size, and required alignment.
layout Header {
  tag:u8
  length:u32
  location:ptr
}

// Equivalent to: mem header:u8(Header.size) align Header.align
mem header:u8(Header)

main: {
  [header + Header.tag]:u8 = 1
  [header + Header.length]:u32 = 1024
  eax = [header + Header.length]:u32
  jmp .valid if eax == 1024
  linux.exit 1

.valid:
  linux.print "Header length is valid\n"
  linux.exit 0
}
