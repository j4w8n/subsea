mem callback:ptr = addr main

main: {
  stack signed:i64 = -7
  stack unsigned:u64 = 7

  rax = -42
  rbx = -1
  rcx = 42
  rdx = 5

  linux.print "inferred signed={}\n", signed
  linux.print "inferred unsigned={}\n", unsigned
  linux.print "inferred ptr={}\n", [callback]
  linux.print "signed={i64}\n", rax
  linux.print "unsigned={u64}\n", rbx
  linux.print "hex={x}\n", rcx
  linux.print "binary={b}\n", rdx
  linux.print "ptr={ptr}\n", rcx

  al = -1
  bl = -1
  cx = -2
  edx = -3
  linux.print "narrow signed={i8} {i16} {i32}\n", al, cx, edx
  linux.print "narrow unsigned={u8}\n", bl

  linux.exit 0
}
