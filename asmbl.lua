
_, bin, deps = task {
  targets = {
    "cargo",
    "cargo/release/asmbl-cli",
    "cargo/release/asmbl-cli.d"
  },
  consumes = "Cargo.toml",
  env = {
    "PATH",
    "RUSTUP_HOME"
  },
  run = "cargo build --release --manifest-path $< --target-dir $@[0]"
}

sub_unit(deps)

task {
  target = "asmbl",
  consumes = bin,
  run = "strip $< -o $@"
}
