# Design

Quartermaster is a package manager built around the part everyone else skips: the resolver. The design goal is that the interesting logic — version algebra, the PubGrub search, and the failure explanation — is small enough to read and audit in a sitting, and correct enough to trust.

## The shape

```
constraints ──▶ range algebra ──▶ PubGrub solver ──▶ resolution
   (text)        (interval set)     (incompatibilities)   or a proof of failure
```

Everything below the CLI is pure and testable without a network or a filesystem. The registry is a trait with two methods; the solver never does IO.

## Versions and ranges

A `Version` is semver: `major.minor.patch` with an optional prerelease, ordered per SemVer 2.0 (a prerelease sorts below its release; prerelease identifiers compare numerically when both are numeric, otherwise lexically, with numeric ranked below alphanumeric).

A `Range` is the crux. It is not a pair of bounds but a **set of versions**: a union of disjoint intervals. The resolver needs `contains`, `intersect`, `union`, `complement`, and `is_empty`, and it needs them to be *total and exact* — no "off by one prerelease" bugs.

The trick is representing each interval endpoint as a **cut**: a position *between* versions rather than a version. `Fin(v, 0)` is the cut immediately before `v`; `Fin(v, 1)` immediately after it. So an inclusive `>=v` and an exclusive `>v` are distinct, comparable points, and all four interval kinds (`[a,b]`, `[a,b)`, `(a,b]`, `(a,b)`) have one representation. Complement, union, and intersection become simple sweeps over sorted cuts instead of a pile of boundary special-cases. This is the single most important design choice: it makes the algebra the solver stands on provably closed under the operations it performs.

A caret/tilde/comparator constraint parses into a `Range`; the reverse, `Range::friendly`, recognises caret and tilde shapes so a resolved `[>=1.2.0, <2.0.0)` prints back as `^1.2.0` in explanations.

## The resolver: PubGrub

The solver is conflict-driven search, the same architecture as a modern CDCL SAT solver, specialised to versions. Two data structures:

- **Incompatibilities**: sets of terms that can never all be true. A dependency "`p` at `v` needs `dep` in `r`" is the incompatibility `{ p = v, not dep in r }`. A dead end is `{ no version of p in r }`. Every other incompatibility is *learned* by resolution.
- **The partial solution**: an ordered log of *decisions* (a chosen version) and *derivations* (a fact forced by unit propagation), each tagged with a decision level and, for derivations, the incompatibility that caused it.

Two moves alternate:

1. **Unit propagation.** For each incompatibility, compare every term to what the partial solution already forces. If all but one term is satisfied and one is undecided, the undecided term's negation is a new derived fact. If *all* terms are satisfied, that is a conflict.
2. **Decision.** Choose a package that some positive requirement forces us to select and that is still undecided (fail-fast: fewest candidate versions first), take its highest allowed version preferring stable releases, and add that version's dependencies as new incompatibilities.

### Conflict resolution and backjumping

When an incompatibility is fully satisfied, the solver does not blindly backtrack one step. It **analyses the satisfier**: the assignment that completed the conflict, and the decision level at which every *other* term of the incompatibility was already satisfied. If the satisfier is a decision, or the two levels differ, it backjumps to that earlier level and the conflicting incompatibility becomes unit there — one clean derivation instead of re-treading dead branches. Otherwise it **resolves**: it builds a new incompatibility from the current one and the satisfier's cause, eliminating the shared package (unioning its two terms, dropping it if that covers every version), and repeats. Learned incompatibilities are kept forever, which is what guarantees termination — the search can never repeat a combination it has already ruled out.

The satisfier analysis (`solver.rs::analyze`) is the subtle heart of the algorithm and the part worth reading twice.

### Why failure is a proof

Resolution stops when it derives an incompatibility that is *terminal*: no terms remain, or the only one left is about the project root, which is fixed. That incompatibility did not appear from nowhere — it was derived from two earlier ones, each derived from two earlier ones, down to leaves that are concrete facts ("the project depends on X", "A 1.0 depends on B", "no version of C matches …"). That derivation tree **is** a proof, and `explain.rs` renders it: a post-order walk emitting "Because … and …, …" lines that terminate in the contradiction. This is why Quartermaster can answer "why not?" when most resolvers only say "no".

## Version selection policy

Given a package and the set of versions still allowed, the solver picks the highest **stable** version, and only falls back to a prerelease when no stable version is in range. This keeps prereleases out of a solution unless a constraint explicitly reaches for one, without threading semver's prerelease-visibility rules through the whole range algebra — the policy lives in one function, not in the set operations.

## Non-goals

No network, no tarballs, no registry protocol — a registry is a text file or an in-memory table, and `install` materializes the resolved set to disk rather than downloading it. No feature flags, optional dependencies, or platform selection. Each of those is real work and each would bury the resolver, which is the whole point of reading this project. What is here — the algebra, the PubGrub core, and the explanations — is complete and correct, not a sketch.
