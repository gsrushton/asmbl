
a = task {
  target = "whoop",
  run = "touch $@"
}

b = task {
  target = "fun!",
  consumes = {"cake"},
  not_before = {a},
  run = {"/bin/bash", "-c", "cat $< > $@"}
}

c = task {
  target = "cheese",
  depends_on = {a},
  not_before = {b},
  run = "touch $@"
}
