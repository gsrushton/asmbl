
bin, deps = task {
  targets = {
    "debug/asmbl-cli",
    "debug/asmbl-cli.d"
  },
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
  consumes = bin,
  run = "strip ${inputs} -o ${target}"
}
