mem values:u64 = [5, 10, 20, 40]

main: {
  rax = 0
  rcx = 0

.loop:
  jmp .done if rcx u>= 4
  rax = rax + values[rcx * 8]
  rcx = rcx + 1
  jmp .loop

.done:
  linux.print "sum={u64}\n", rax
  linux.exit 0
}
