import first from "multi_export.ss"
import second from "multi_export.ss"

main: {
  call first
  call second
  linux.exit 0
}
