layout Point align 8 {
  x:i64
  y:i64
}

mem point:u8(Point)

add: (rdi:u64, rsi:u64) -> rax:u64 [rcx, r8] {
  rax = rdi + rsi
  ret
}

main: {
  [point + Point.x]:i64 = 10
  [point + Point.y]:i64 = 20
  rdi = [point + Point.x]:u64
  rsi = [point + Point.y]:u64
  call add
  linux.print rax
  linux.print "\n"
  linux.exit 0
}
