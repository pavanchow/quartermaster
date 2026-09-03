//! An `Incompatibility` is a set of terms that can never all be true at once.
//! Two shapes seed the search and the rest are *derived* by resolution:
//!
//! - a dependency: "package p at version v requires dep in range r" becomes the
//!   incompatibility { p = v, not dep in r } (you cannot pick v and also refuse
//!   every version of dep it needs);
//! - a dead end: "no published version of p is in range r".
//!
//! Each one remembers *why* it exists (`Cause`) so a failed resolution can be
//! replayed as a human-readable proof. There is at most one term per package.
use crate::term::Term;

pub type IncompatId = usize;

#[derive(Clone)]
pub enum Cause {
    /// The synthetic root package that carries the project's own dependencies.
    Root,
    /// `package`@`version` depends on `dep` within a range (the two terms).
    Dependency { package: String, version: String, dep: String },
    /// No published version of `package` falls in the required range.
    NoVersions { package: String },
    /// Learned by resolving two earlier incompatibilities during a conflict.
    Derived(IncompatId, IncompatId),
}

#[derive(Clone)]
pub struct Incompatibility {
    /// Terms keyed by package, at most one per package.
    pub terms: Vec<(String, Term)>,
    pub cause: Cause,
}

impl Incompatibility {
    pub fn new(terms: Vec<(String, Term)>, cause: Cause) -> Incompatibility {
        Incompatibility { terms, cause }
    }

    pub fn term_for(&self, package: &str) -> Option<&Term> {
        self.terms.iter().find(|(p, _)| p == package).map(|(_, t)| t)
    }

    pub fn packages(&self) -> impl Iterator<Item = &String> {
        self.terms.iter().map(|(p, _)| p)
    }

    /// A "terminal" incompatibility means the resolution has bottomed out at the
    /// project's own requirements: either no terms remain, or the only one left
    /// is about the root itself, which is fixed. Either way, unsatisfiable.
    pub fn is_terminal(&self, root: &str) -> bool {
        self.terms.is_empty() || (self.terms.len() == 1 && self.terms[0].0 == root)
    }
}
