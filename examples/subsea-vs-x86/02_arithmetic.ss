main: {
  rax = 10
  rax = rax + 5
  rax = rax - 1

  rbx = (rax * 3) + 2
  rcx = rbx i> 40

  rdx = 11
  rdx = 99 if rbx i> 40

  linux.print "value={i64}, greater_than_40={u64}, chosen={i64}\n", rbx, rcx, rdx
  linux.exit 0
}
