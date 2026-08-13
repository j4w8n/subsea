main: {
  rax = 257
  al = rax
  rbx = al::zx

  linux.print rbx
  linux.print "\n"

  linux.exit 0
}
