
cargo = task {
  target = "debug/asmbl-cli",
  consumes = "Cargo.toml",
  env = {
    "PATH",
    "RUSTUP_HOME"
  },
  run = "cargo build --manifest-path ${inputs} --target-dir ."
}



sub_unit("debug/asmbl-cli.d", true)

task {
  target = "asmbl",
  consumes = cargo,
  run = "strip ${inputs} -o ${target}"
}
