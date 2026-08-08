main: {
  const message = "Hello World!\n"
  print message

  print "Printed directly!\n"

  jmp end
}

end: {
  print "jmp works!\n"

  const count = 6
  print "count = {}\n", count

  exit 0
}
