export mem shared:u64 = 42
mem hidden:u64 = 7

export data metadata section ".metadata" {
  u64 1
}

data hidden_metadata section ".metadata" {
  u64 2
}
