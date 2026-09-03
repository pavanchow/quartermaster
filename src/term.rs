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
