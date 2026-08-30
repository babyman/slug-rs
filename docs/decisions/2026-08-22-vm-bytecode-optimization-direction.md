# Adopt a measured private-bytecode optimization direction

## Context

The initial Rust VM intentionally uses typed instructions, an operand stack,
source spans on each instruction, and binding cells for every frame local. That
representation keeps the early compiler and checked runtime straightforward,
but it also clones instruction-owned data during dispatch, repeats source-path
storage, and pays reference-counting and interior-mutability costs for locals
that no closure captures.

Changing directly to a register VM would combine those representation costs
with a more complex temporary allocator without establishing which costs
matter in representative Slug programs. The private bytecode remains free to
change, so these costs can be removed before selecting a different operand
model.

## Decision

Optimize the existing stack VM before considering a register conversion. Work
proceeds through the staged plan in [`../engineering/vm-optimization.md`](../engineering/vm-optimization.md):

1. dispatch borrows instructions and clones diagnostic or opcode-owned data
   only when ownership is required;
2. executable instructions refer to interned source and metadata tables by
   compact identifiers;
3. ordinary frame locals store `Value` directly, while locals captured by a
   closure are promoted to shared binding cells; and
4. representative benchmarks determine whether a register operand model is
   justified.

Private bytecode uses a small, regular instruction core with selectively
complex semantic operations. Arithmetic, movement, comparison, and control
flow remain regular operations. Calls, closure creation, collection
construction, pattern matching, deferred cleanup, throwing, and recurrence may
remain medium-grained operations because each represents a meaningful runtime
boundary. Variable-size descriptors such as patterns, capture lists, field
lists, names, and source locations live in indexed metadata pools instead of
inside executable instructions.

Do not add combinations of otherwise independent operations solely to reduce
dispatch. A fused instruction or superinstruction requires benchmark evidence
from a recurring hot sequence and must preserve checked bytecode validation.

The stack and register operand models remain private implementation choices. A
future register VM requires a separate decision supported by measurements of
instruction count, execution time, bytecode size, value cloning, and compiler
complexity. Neither private representation is the portable `.cslug`
instruction set.

## Consequences

- Near-term work removes known allocation, cloning, and metadata costs without
  increasing compiler control-flow complexity.
- Shared mutable captures retain their source-level identity while ordinary
  locals avoid unconditional `Rc<RefCell<_>>` storage.
- Compact instruction encoding is treated separately from the stack-versus-
  register decision.
- Complex language operations do not have to be decomposed into dispatch-heavy
  micro-operations merely to make the instruction set uniformly RISC-like.
- The VM avoids an unbounded CISC-style family of operand and operation
  combinations.
- Benchmark infrastructure and capture-aware local storage add implementation
  work before a register VM can be evaluated fairly.
- A later register conversion remains possible because private bytecode is not
  a compatibility promise.

## Migration

No Slug source migration is required. Private `Instruction`, `Op`, frame,
capture, and direct bytecode-test APIs may change as each stage is implemented.
