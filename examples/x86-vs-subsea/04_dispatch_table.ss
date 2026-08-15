mem handlers:ptr = [addr add_ten, addr double_value, addr subtract_three]

main: {
  rdi = 7
  rax = 1
  call handlers[rax * 8]:ptr

  linux.print "result={u64}\n", rax
  linux.exit 0
}

add_ten: {
  rax = rdi + 10
  ret
}

double_value: {
  rax = rdi + rdi
  ret
}

subtract_three: {
  rax = rdi - 3
  ret
}
