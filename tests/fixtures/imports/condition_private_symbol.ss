mem flag:u64 = 0

export check_flag: {
  jmp .done if [flag]:u64 == 0
  ret

.done:
  ret
}
