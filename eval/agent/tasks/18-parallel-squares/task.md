# Parallel squares

Read a list of integers with `cfg("numbers", [3, 1, 4, 1, 5])`. Start one concurrent task per number that computes its square, then wait for every task. Print the list of squares in the original order, using Slug's normal list display, then on a second line print their sum.
