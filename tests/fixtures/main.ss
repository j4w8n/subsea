main: {
  const message = "Hello World!\n"
  linux.print message

  linux.print "Printed directly!\n"

  stack stack_message:str = "Hello from the stack!\n"
  linux.print stack_message

  linux.exit 0
}
