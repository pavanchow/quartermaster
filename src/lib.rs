//! Quartermaster: a small package manager whose interesting half is a readable
//! dependency resolver. Given a set of top-level constraints and a package
//! registry, [`resolve`] either returns an exact version for every transitive
//! dependency or a plain-English proof that no such set exists.
//!
//! The resolver ([`solver`]) is a from-scratch PubGrub implementation; the
//! version and range algebra it stands on ([`version`], [`range`]) are their
//! own readable pieces, and a failed resolution is rendered by [`explain`].
pub mod error;
pub mod explain;
pub mod incompat;
pub mod install;
pub mod lock;
pub mod manifest;
pub mod range;
pub mod registry;
pub mod solver;
pub mod term;
pub mod version;

pub use error::{Error, Result};
pub use range::Range;
pub use registry::{Provider, Registry};
pub use version::Version;

use solver::{Outcome, Solver};
use std::collections::BTreeMap;

/// The outcome of resolving a dependency set.
pub enum Resolved {
    /// Every package pinned to an exact version.
    Ok(BTreeMap<String, Version>),
    /// No solution exists; the string is a human-readable proof of why.
    Conflict(String),
}

/// Resolve `root_deps` against `provider`. `root_deps` are the project's own
/// direct dependencies as (name, constraint) pairs.
pub fn resolve(
    provider: &dyn Provider,
    root_deps: Vec<(String, Range)>,
) -> std::result::Result<Resolved, String> {
    let mut solver = Solver::new(provider);
    match solver.solve(root_deps)? {
        Outcome::Solved(res) => Ok(Resolved::Ok(res.packages)),
        Outcome::Unsolvable(id) => Ok(Resolved::Conflict(explain::report(&solver.incompats, id))),
    }
}
