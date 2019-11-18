
bin, deps = task {
  targets = {
    "release/asmbl-cli",
    "release/asmbl-cli.d"
  },
  consumes = "Cargo.toml",
  env = {
    "PATH",
    "RUSTUP_HOME"
  },
  run = "cargo build --manifest-path $< --target-dir . --release"
}

sub_unit(deps)

task {
  target = "asmbl",
  consumes = bin,
  run = "strip $< -o $@"
}
