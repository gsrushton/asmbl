
obj, deps = task {
  targets = {
    "%f.o",
    "%f.d"
  },
  consumes = {"src/main.c", "src/name.c"},
  env = {
    "PATH"
  },
  run = "gcc -o $@[0] -c $< -Iinclude -MMD -MT $@[0] -MF $@[1]"
}

include(deps)

bin = task {
  target = "example",
  consumes = obj,
  env = {
    "PATH"
  },
  run = "gcc -o $@ $<"
}
