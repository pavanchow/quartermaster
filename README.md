# Quartermaster

**A package manager whose interesting half is a readable dependency resolver — a from-scratch [PubGrub](https://nex3.medium.com/pubgrub-2fb6470504f) version solver that explains a conflict in plain English instead of just failing.** Give it a set of version constraints and a registry and it returns an exact version for every transitive dependency, or a step-by-step proof that no such set exists. Zero dependencies, one small binary. By **Pavan Nallamothu** ([`pavanchow`](https://github.com/pavanchow)).

## Why this exists

Every "build your own package manager" tutorial stops at the easy parts — a manifest format, a lockfile, copying files — and hand-waves the one part that is actually hard: **dependency resolution**. Picking a single version of every package such that every constraint holds is NP-hard in general, and doing it well is what separates a real package manager from a toy.

And the tools that do resolve have a second problem: when they *can't*, they tell you almost nothing. `npm` and `pip` are notorious for dumping a wall of version numbers and leaving you to reverse-engineer which two requirements actually collide.

Quartermaster is built around exactly those two things:

- **A real resolver.** A faithful PubGrub implementation with unit propagation, conflict-driven clause learning, and backjumping — the same algorithm shape as a modern SAT solver, and what Dart's `pub` and `uv` use.
- **Conflict explanations you can read.** When resolution fails, you get the derivation path, phrased in English:

  ```
  Because http 1.2.0 depends on bytes ^1.1.0 and the project depends on bytes 1.0.0,
  http ^1.2.0 cannot be used.
  Because http ^1.2.0 cannot be used and the project depends on http ^1.2.0,
  the project's requirements are contradictory.
  ```

That is the gap it fills, for a person debugging a lockfile *and* for an AI agent that proposes a dependency set and needs a machine-checkable reason when it doesn't hold — not "resolution failed", but which two requirements to change.

## What makes it different

- **The resolver is the product**, not an afterthought. The whole thing is a few hundred readable lines you can audit in a sitting.
- **Failures are proofs.** Most from-scratch package managers cannot explain a conflict at all. This one treats the explanation as a first-class output.
- **Boundary-exact version algebra.** Ranges (`^1.2`, `~1.2`, `>=1.0, <2.0`, `1 || 2`) are a real interval set with total intersection / union / complement, so the solver reasons correctly instead of string-matching versions.
- **No dependencies, portable.** Pure Rust standard library. Nothing to install, nothing to trust.

## Quickstart

```sh
cargo build
cargo test

# resolve a project against a registry
./target/debug/qm resolve examples/app.qm --registry examples/registry.qm
./target/debug/qm tree    examples/app.qm --registry examples/registry.qm
./target/debug/qm lock    examples/app.qm --registry examples/registry.qm
./target/debug/qm explain examples/conflict.qm --registry examples/registry.qm
```

## The manifest and the registry

A **manifest** names a project and lists its direct dependencies:

```text
name    myapp
version 0.1.0
require web  ^1.0
require json ^1.2
```

A **registry** is the universe of available versions — one package version per stanza, its dependencies indented:

```text
web 1.1.0
  http ^1.2
  json ^1.2
http 1.3.0
  bytes ^1.1
bytes 1.1.5
```

## What you get back

```
$ qm resolve examples/app.qm --registry examples/registry.qm
resolved myapp 0.1.0 (4 packages):
  bytes 1.1.5
  http 1.3.0
  json 1.3.0
  web 1.1.0
```

```
$ qm tree examples/app.qm --registry examples/registry.qm
myapp 0.1.0
├─ json 1.3.0
│  └─ bytes 1.1.5
└─ web 1.1.0
   ├─ http 1.3.0
   │  └─ bytes 1.1.5 (*)
   └─ json 1.3.0 (*)
```

`(*)` marks a package already shown above, so a shared dependency is printed once.

## Constraints it understands

| Form | Means |
|------|-------|
| `1.2.3` | exactly 1.2.3 |
| `^1.2.3` | `>=1.2.3, <2.0.0` (`^0.2.3` is `>=0.2.3, <0.3.0`) |
| `~1.2.3` | `>=1.2.3, <1.3.0` |
| `>=1.0, <2.0` | intersection of comparators |
| `1 \|\| 2` | union (either major) |
| `*` | any published version |

Versions are semver: `major.minor.patch` with optional prerelease (`1.0.0-rc.1`), ordered per SemVer 2.0. The resolver prefers stable releases and only picks a prerelease when nothing stable satisfies a constraint.

## How the resolver works

Two moves alternate over a growing set of *incompatibilities* (combinations of versions that can't coexist):

1. **Unit propagation** — when an incompatibility has all but one term already forced, the last term's negation becomes a new derived fact.
2. **Decision** — pick a still-required package, try its highest allowed version, and add that version's dependencies as new incompatibilities.

When an incompatibility becomes fully satisfied, that is a conflict: it is *resolved* against the fact that caused it, learning a new incompatibility and backjumping to where the new one becomes unit. If resolution bottoms out at the project's own requirements, there is no solution — and the terminal incompatibility is the proof [`explain`](src/explain.rs) renders. See [DESIGN.md](DESIGN.md).

## Layout

| File | What |
|------|------|
| [`version.rs`](src/version.rs) | Semver versions and ordering |
| [`range.rs`](src/range.rs) | The interval-set version algebra |
| [`term.rs`](src/term.rs) · [`incompat.rs`](src/incompat.rs) | The atoms the solver reasons with |
| [`solver.rs`](src/solver.rs) | The PubGrub resolver |
| [`explain.rs`](src/explain.rs) | Failure rendered as a proof |
| [`registry.rs`](src/registry.rs) · [`manifest.rs`](src/manifest.rs) · [`lock.rs`](src/lock.rs) · [`install.rs`](src/install.rs) | The package-manager plumbing |

## License

MIT.
