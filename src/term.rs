//! A `Term` is a statement about a single package: its version is in a range
//! (positive) or not in a range (negative). Terms are the atoms the resolver
//! reasons with. The only operations it needs are the *allowed set* (the
//! versions that make the term true), negation, and the three-valued relation
//! between what we've assumed so far and what a term requires.
use crate::range::Range;
use crate::version::Version;

#[derive(Clone, PartialEq, Eq)]
pub struct Term {
    pub positive: bool,
    pub range: Range,
}

/// How an accumulated assumption (a set of still-possible versions) stands
/// against a term.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Relation {
    /// Every possible version already makes the term true.
    Satisfied,
    /// No possible version can make the term true.
    Contradicted,
    /// Some can, some cannot: undecided.
    Inconclusive,
}

impl Term {
    pub fn positive(range: Range) -> Term {
        Term { positive: true, range }
    }
    pub fn negative(range: Range) -> Term {
        Term { positive: false, range }
    }
    pub fn exact(v: Version) -> Term {
        Term::positive(Range::exact(v))
    }

    /// The set of versions for which this term is true.
    pub fn allowed(&self) -> Range {
        if self.positive {
            self.range.clone()
        } else {
            self.range.complement()
        }
    }

    pub fn negate(&self) -> Term {
        Term { positive: !self.positive, range: self.range.clone() }
    }

    /// A term ranges over *package states*: a concrete version, or the package
    /// being absent. A negative term is satisfied by absence; a positive term is
    /// not. `intersect`/`union` combine terms as sets of those states, tracking
    /// whether absence survives so the collapse to a bare version range does not
    /// silently drop it.
    fn covers_absent(&self) -> bool {
        !self.positive
    }

    fn from_parts(versions: Range, absent: bool) -> Term {
        if absent {
            Term::negative(versions.complement())
        } else {
            Term::positive(versions)
        }
    }

    pub fn intersect(&self, other: &Term) -> Term {
        let versions = self.allowed().intersect(&other.allowed());
        Term::from_parts(versions, self.covers_absent() && other.covers_absent())
    }

    pub fn union(&self, other: &Term) -> Term {
        let versions = self.allowed().union(&other.allowed());
        Term::from_parts(versions, self.covers_absent() || other.covers_absent())
    }

    /// True when every possible state (absence or any version) satisfies the
    /// term, so it constrains nothing and can be dropped from an incompatibility.
    pub fn is_universal(&self) -> bool {
        self.covers_absent() && self.allowed().complement().is_empty()
    }

    /// No state at all satisfies the term (positive over an empty range).
    fn is_empty_state(&self) -> bool {
        self.positive && self.range.is_empty()
    }

    /// The accumulated state that constrains nothing yet: absence or any version.
    pub fn any_state() -> Term {
        Term::negative(Range::empty())
    }

    /// Whether every state allowed by `self` is also allowed by `other`.
    fn subset_of(&self, other: &Term) -> bool {
        (other.covers_absent() || !self.covers_absent())
            && self.allowed().intersect(&other.allowed()) == self.allowed()
    }

    /// Relation of an accumulated per-package state `acc` (the set of states the
    /// package can still take) to this term.
    pub fn relation_to(&self, acc: &Term) -> Relation {
        if acc.subset_of(self) {
            Relation::Satisfied
        } else if acc.intersect(self).is_empty_state() {
            Relation::Contradicted
        } else {
            Relation::Inconclusive
        }
    }

    /// Relation of an accumulated allowed set `have` (the versions still
    /// possible for this package) to this term.
    pub fn relation(&self, have: &Range) -> Relation {
        let want = self.allowed();
        let inside = have.intersect(&want);
        if inside == *have {
            Relation::Satisfied
        } else if inside.is_empty() {
            Relation::Contradicted
        } else {
            Relation::Inconclusive
        }
    }
}
