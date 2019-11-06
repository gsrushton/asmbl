
a = task {
  target = "whoop",
  run = "touch ${target}"
}

b = task {
  target = "fun!",
  consumes = {"cake"},
  not_before = {a},
  run = {"/bin/bash", "-c", "cat ${inputs} > ${target}"}
}

c = task {
  target = "cheese",
  depends_on = {a},
  not_before = {b},
  run = "touch ${target}"
}
