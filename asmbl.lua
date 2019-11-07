
cargo = task {
  target = "target/debug/asmbl-cli",
  env = {
    "PATH",
    "RUSTUP_HOME"
  },
  run = {"cargo", "build"}
}

task {
  target = "target/debug/asmbl",
  consumes = cargo,
  run = {"strip", "${inputs}", "-o", "${target}"}
}
