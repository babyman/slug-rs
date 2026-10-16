# Manhattan perimeter

Read a polygon as a list of `[x, y]` integer vertices with `cfg("points", [[0, 0], [4, 0], [4, 3], [0, 3]])`. Represent each vertex as a struct with fields `x` and `y`. Print the perimeter measured with Manhattan distance (`|dx| + |dy|`), including the edge from the last vertex back to the first. A single vertex has perimeter `0`.
