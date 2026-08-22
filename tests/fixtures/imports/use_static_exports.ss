import shared, metadata from "static_exports.ss"

main: {
  rax = [shared]
  rbx = &metadata
  linux.exit 0
}
