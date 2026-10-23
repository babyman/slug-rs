# Safe divide

Read a list of `[a, b]` integer pairs with `cfg("pairs", [[10, 2], [7, 0], [9, 3]])`. Write a function that divides `a` by `b`, truncating toward zero, and throws an error with message `division by zero` when `b` is 0. For each pair, in order, print the quotient, or, if the function threw, recover and print `error: division by zero`. Process every pair even after an error.
