main: {
  rdi = 42
  rsi = 17
  call add

  linux.print "Result is {u64}\n", rcx

  linux.exit 0
}

add: {
  rcx = rdi + rsi
  ret
}
