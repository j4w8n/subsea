layout Header {
  tag:u8
  length:u32
  location:ptr
}

mem header:u8(Header)

main: {
  [header + Header.tag]:u8 = 1
  [header + Header.length]:u32 = 1024
  eax = [header + Header.length]:u32
  jmp .valid if eax == 1024
  linux.exit 1

.valid:
  linux.exit 0
}
