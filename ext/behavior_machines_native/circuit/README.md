# behavior-machines-circuit

Compiles a [behavior-machines](../core) tree into a branchless arithmetic
circuit. One tick becomes a fixed, layered evaluation of affine forms and
rectified gates instead of a recursive walk, so the work a tick does no longer
depends on which branch it takes.

```rust
use behavior_machines::{Node, Status};
use behavior_machines_circuit::{Lowered, State};

// A reactive fallback: charge while the battery is low, otherwise patrol.
let tree = Node::reactive_fallback([
    Node::sequence([Node::leaf("battery-low"), Node::leaf("charge")]),
    Node::leaf("patrol"),
]);

let lowered = Lowered::new(&tree)?;
let mut state = State::initial(&lowered);

// One status per leaf, in leaf order, evaluated by the host.
let schedule = lowered.evaluate(&mut state, &[
    Status::Failure, // battery-low
    Status::Success, // charge
    Status::Running, // patrol
])?;

assert_eq!(schedule.root(), Some(Status::Running));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What it is for

**Not speed on a single tree.** The engine's walk is already a few hundred
lines of flat Rust over a slot arena; a tens-of-nodes tree evaluated as a
circuit is a wash at best. Three things do pay:

- **A batch axis.** The circuit is the same shape for every evaluation, so *N*
  agents sharing one tree is one wide evaluation rather than *N* walks. This
  is where the prior art finds its wins, and it needs *N* to be large.
- **Constant-time ticks.** A walk's duration depends on the path taken; the
  circuit's is a fixed number of layers. For a real-time or embedded host with
  a deadline to prove, a worst case that equals the average case is worth
  paying for.
- **A differentiable tree.** Affine forms and rectified gates are what a
  neural net is made of, so the arbitration has a gradient. Freezing the
  structure while training the thresholds — a `Parallel` threshold that tunes
  itself, a `Fallback` that learns its priority order — is the interesting
  experiment, and the published work on trainable trees is all on *decision*
  trees, not behavior trees.

A fourth, immediate benefit: the circuit backend has to agree with the engine
on every tick, which makes it a test that arbitration is pure and that effects
live only in leaves.

## Every leaf must be pure

The circuit needs every leaf's status before it can say which leaves the tick
reaches, so the host evaluates all of them up front. That is only sound when
ticking a leaf has no effect beyond its returned status — a condition, not an
action.

This is forced, not an oversight. An ordered composite ticks its next child
only if the previous one succeeded, *within the same tick*, so one circuit
evaluation cannot both sequence effects and compute the arbitration. Supporting
effectful actions means splitting leaves into conditions and actions at compile
time and having the circuit pick an action set the host then runs. That is a
different interface and it is not built yet.

## Supported nodes

Lowered: `Leaf`, `AlwaysSuccess`, `AlwaysFailure`, `Sequence`, `Fallback`,
`ReactiveSequence`, `ReactiveFallback`, `Inverter`, `ForceSuccess`,
`ForceFailure`.

Everything else returns `LowerError::Unsupported` with the node's name:
`Parallel`, `IfThenElse`, `WhileDoElse`, `Sleep`, `Timeout`, `Repeat`,
`RetryUntilSuccessful`, `KeepRunningUntilFailure`. `Node` is
`#[non_exhaustive]`, so a node added to the engine later lands there too
rather than compiling to something wrong by accident.

The timed nodes need care beyond arithmetic: the engine's clock is `u64`
nanoseconds and `f32` holds integers exactly only below 2^24, so a deadline
cannot enter a circuit as an absolute time. They will take **relative**
deadlines, with `next_deadline` staying integer math on the host.

## How it works

Three passes, in [`lower`]:

1. **Flatten** the definition in the engine's pre-order, so a node's index is
   its `NodeId` and every descendant sits at a higher index.
2. **Arbitrate** bottom-up: each node's *raw* status, meaning what it would
   return if it were ticked. This depends only on children's raw statuses and
   the node's own state, never on whether the node is reached.
3. **Reach** top-down: which nodes the tick visits, and which running nodes it
   halts.

Separating "what would it return" from "is it reached" is what removes the
recursion. A walk interleaves the two; split apart, each becomes a layer of
arithmetic.

Both ordered composites reduce to one identity — the walk passes over a child
or stops at it:

```text
before_i  = [i < cursor]                  the cursor already passed this child
blocked_i = NOT (before_i OR advance_i)   the walk cannot get past child i
clear_i   = [Σ_{j<i} blocked_j == 0]      every earlier child was passed
reach_i   = clear_i AND NOT before_i      so this child is ticked
stop_i    = clear_i AND blocked_i         and the walk stops here
```

`blocked` depends only on the cursor and each child's own status, never on the
other children, so the whole vector exists before any prefix test runs. `clear`
is then one `none_earlier` call shared by both answers, and `Σ blocked_j` is a
non-negative integer, so the test against zero is a **single** rectified gate,
`max(0, 1 - Σ)`. `stop` is at-most-one-hot by construction, so every collapse
over it is plain addition and costs no operations at all. A reactive composite
is the same lowering with the cursor pinned to zero, which constant-folds
`before_i` away.

The consequence is the measured cost, which `lower/tests.rs` pins:

| | depth | operations per child |
|---|---|---|
| reactive composite | 5, any width | 8 |
| memory composite | 7, any width | 12 |

**Depth does not grow with the number of children.** A prefix test reads a sum
of bits and a sum is free, so a wider composite widens a layer rather than
adding one — four children and sixty-four children cost the same number of
sequential steps. Depth grows only with nesting, about two layers per level.

The status-rewrite decorators are free: `Inverter` relabels two wires and the
`Force` pair merges two disjoint bits.

## Exactness

Every value in a lowered tree is an integer — a bit, a child index, or a count
of children — and all of them stay far below 2^24, where `f32` is exact. No
operation ever rounds, which is checked under `debug_assertions`.

This is a property of the lowering rather than a tolerance. Note what it rests
on: conditions are compared by the *host*, at full precision, and enter the
circuit already decided as bits. The circuit never compares a float, so there
is no threshold for a rounding error to land on.

## License

BSD-3-Clause, as the rest of the repository.
