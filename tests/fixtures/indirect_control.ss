mem callback:ptr = addr print_callback
mem exits:ptr = [addr done_zero, addr done_one]

main: {
  rax = [callback]
  call rax

  rax = 1
  jmp exits[rax * 8]:ptr
}

print_callback: {
  linux.print "called\n"
  ret
}

done_zero: {
  linux.exit 2
}

done_one: {
  linux.print "jumped\n"
  linux.exit 0
}
