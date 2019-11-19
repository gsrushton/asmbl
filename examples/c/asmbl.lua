
obj, deps = task {
  targets = {
    "main.o",
    "main.d"
  },
  consumes = "src/main.c",
  env = {
    "PATH"
  },
  run = "gcc -o $@[0] -c $< -Iinclude -MMD -MP -MT $@[0] -MF $@[1]"
}

sub_unit(deps)

bin = task {
  target = "example",
  consumes = obj,
  env = {
    "PATH"
  },
  run = "gcc -o $@ $<"
}
