//! The resolver: a from-scratch implementation of the PubGrub algorithm.
//!
//! The idea is conflict-driven search, the same shape as a modern SAT solver.
//! We keep a growing set of *incompatibilities* (combinations of versions that
//! cannot coexist) and a *partial solution* (an ordered log of decisions and
//! derived facts). Two moves alternate:
//!
//! - **unit propagation**: whenever an incompatibility has all-but-one of its
//!   terms already forced, the last term's negation is a new derived fact;
//! - **decision**: pick a still-required package and try its best version,
//!   adding that version's dependencies as new incompatibilities.
//!
//! When an incompatibility becomes fully satisfied that is a conflict. We then
//! *resolve* it against the fact that caused it, learning a new incompatibility
//! and backjumping to where that new one becomes unit. If resolution bottoms
//! out at the project's own requirements, there is no solution, and the
//! terminal incompatibility is a machine-checkable proof of why (see
//! [`crate::explain`]).
use crate::incompat::{Cause, IncompatId, Incompatibility};
use crate::range::Range;
use crate::registry::Provider;
use crate::term::{Relation, Term};
use crate::version::Version;
use std::collections::{BTreeMap, HashMap};

pub const ROOT: &str = "@root";

pub struct Resolution {
    pub packages: BTreeMap<String, Version>,
}

pub enum Outcome {
    Solved(Resolution),
    /// Unsolvable: the id of the terminal incompatibility, resolvable to prose.
    Unsolvable(IncompatId),
}

struct Assignment {
    package: String,
    term: Term,
    decision: bool,
    level: usize,
    cause: Option<IncompatId>,
}

pub struct Solver<'a> {
    provider: &'a dyn Provider,
    pub incompats: Vec<Incompatibility>,
    index: HashMap<String, Vec<IncompatId>>,
    assignments: Vec<Assignment>,
    level: usize,
    root_version: Version,
}

enum Eval {
    Conflict,
    Unit(String, Term),
    Ignore,
}

enum Conflict {
    NoSolution(IncompatId),
    Learned(String),
}

impl<'a> Solver<'a> {
    pub fn new(provider: &'a dyn Provider) -> Solver<'a> {
        Solver {
            provider,
            incompats: Vec::new(),
            index: HashMap::new(),
            assignments: Vec::new(),
            level: 0,
            root_version: Version::new(1, 0, 0),
        }
    }

    fn add_incompat(&mut self, terms: Vec<(String, Term)>, cause: Cause) -> IncompatId {
        let id = self.incompats.len();
        for (p, _) in &terms {
            self.index.entry(p.clone()).or_default().push(id);
        }
        self.incompats.push(Incompatibility::new(terms, cause));
        id
    }

    /// Versions still possible for `pkg` given everything derived so far.
    fn term_for(&self, pkg: &str) -> Range {
        self.acc_for(pkg, self.assignments.len())
    }

    /// Accumulated allowed set for `pkg` over the first `upto` assignments.
    fn acc_for(&self, pkg: &str, upto: usize) -> Range {
        let mut acc = Range::any();
        for a in &self.assignments[..upto] {
            if a.package == pkg {
                acc = acc.intersect(&a.term.allowed());
            }
        }
        acc
    }

    fn decision(&self, pkg: &str) -> Option<Version> {
        self.assignments
            .iter()
            .find(|a| a.decision && a.package == pkg)
            .map(|a| a.term.range.lower_version().cloned().unwrap())
    }

    fn eval(&self, ci: IncompatId) -> Eval {
        let mut inconclusive: Option<(String, Term)> = None;
        let mut count = 0;
        for (pkg, term) in &self.incompats[ci].terms {
            let have = self.term_for(pkg);
            match term.relation(&have) {
                Relation::Satisfied => {}
                Relation::Contradicted => return Eval::Ignore,
                Relation::Inconclusive => {
                    count += 1;
                    inconclusive = Some((pkg.clone(), term.clone()));
                }
            }
        }
        match count {
            0 => Eval::Conflict,
            1 => {
                let (p, t) = inconclusive.unwrap();
                Eval::Unit(p, t)
            }
            _ => Eval::Ignore,
        }
    }

    fn derive(&mut self, pkg: &str, term: Term, cause: IncompatId) {
        self.assignments.push(Assignment {
            package: pkg.to_string(),
            term,
            decision: false,
            level: self.level,
            cause: Some(cause),
        });
    }

    fn decide(&mut self, pkg: &str, version: Version) {
        self.level += 1;
        self.assignments.push(Assignment {
            package: pkg.to_string(),
            term: Term::exact(version),
            decision: true,
            level: self.level,
            cause: None,
        });
    }

    fn backtrack(&mut self, to_level: usize) {
        self.assignments.retain(|a| a.level <= to_level);
        self.level = to_level;
    }

    /// Smallest index `i < upto` at which `pkg`'s accumulated set first
    /// satisfies `term`, scanning only `pkg`'s assignments.
    fn satisfier_of(&self, pkg: &str, term: &Term, upto: usize) -> Option<usize> {
        let mut acc = Range::any();
        for i in 0..upto {
            if self.assignments[i].package == pkg {
                acc = acc.intersect(&self.assignments[i].term.allowed());
                if term.relation(&acc) == Relation::Satisfied {
                    return Some(i);
                }
            }
        }
        None
    }

    /// Locate the assignment that completes a satisfied incompatibility, and the
    /// decision level to backjump to (the level at which every *other* term is
    /// already satisfied, leaving the satisfier's term as the lone unit).
    fn analyze(&self, ci: IncompatId) -> (usize, usize) {
        let terms = self.incompats[ci].terms.clone();
        let n = self.assignments.len();
        let mut sat_index = 0;
        for (pkg, term) in &terms {
            if let Some(idx) = self.satisfier_of(pkg, term, n) {
                sat_index = sat_index.max(idx);
            }
        }
        let satisfier_pkg = self.assignments[sat_index].package.clone();
        let mut prev = 1usize;
        for (pkg, term) in &terms {
            if *pkg == satisfier_pkg {
                if let Some(j) = self.satisfier_of(pkg, term, sat_index) {
                    prev = prev.max(self.assignments[j].level);
                }
            } else if let Some(idx) = self.satisfier_of(pkg, term, n) {
                prev = prev.max(self.assignments[idx].level);
            }
        }
        (sat_index, prev)
    }

    /// Resolve `ci` against the cause of its satisfier on `pkg`, learning a new
    /// incompatibility with `pkg` eliminated (its two terms unioned; dropped if
    /// that covers every version).
    fn resolve_incompat(&mut self, ci: IncompatId, cause: IncompatId, pkg: &str) -> IncompatId {
        let mut merged: Vec<(String, Term)> = Vec::new();
        let mut push = |q: &str, t: &Term| {
            if q == pkg {
                return;
            }
            if let Some(slot) = merged.iter_mut().find(|(p, _)| p == q) {
                let both = slot.1.allowed().intersect(&t.allowed());
                slot.1 = Term::positive(both);
            } else {
                merged.push((q.to_string(), t.clone()));
            }
        };
        for (q, t) in &self.incompats[ci].terms {
            push(q, t);
        }
        for (q, t) in &self.incompats[cause].terms {
            push(q, t);
        }
        // Union the two terms for the eliminated package; keep it only if the
        // union is not "every version" (which would make it vacuous).
        let cp = self.incompats[ci].term_for(pkg).map(|t| t.allowed());
        let dp = self.incompats[cause].term_for(pkg).map(|t| t.allowed());
        let combined = match (cp, dp) {
            (Some(a), Some(b)) => Some(a.union(&b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        };
        if let Some(u) = combined {
            if !u.complement().is_empty() {
                merged.push((pkg.to_string(), Term::positive(u)));
            }
        }
        self.add_incompat(merged, Cause::Derived(ci, cause))
    }

    fn conflict_resolution(&mut self, mut ci: IncompatId) -> Conflict {
        loop {
            if self.incompats[ci].is_terminal(ROOT) {
                return Conflict::NoSolution(ci);
            }
            let (sat_index, prev_level) = self.analyze(ci);
            let sat = &self.assignments[sat_index];
            let sat_pkg = sat.package.clone();
            let sat_level = sat.level;
            let sat_is_decision = sat.decision;
            if sat_is_decision || prev_level != sat_level {
                self.backtrack(prev_level);
                return Conflict::Learned(sat_pkg);
            }
            let cause = self.assignments[sat_index].cause.expect("derivation has a cause");
            ci = self.resolve_incompat(ci, cause, &sat_pkg);
        }
    }

    /// Undecided packages that some positive requirement forces us to select.
    fn required_undecided(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for a in &self.assignments {
            if a.term.positive && a.package != ROOT && !seen.contains(&a.package) {
                seen.push(a.package.clone());
            }
        }
        seen.retain(|p| self.decision(p).is_none());
        seen
    }

    /// Highest version of `pkg` inside `allowed`, preferring stable releases
    /// over prereleases even when a prerelease sorts higher.
    fn choose_version(&self, pkg: &str, allowed: &Range) -> Option<Version> {
        let mut matching: Vec<Version> =
            self.provider.versions(pkg).into_iter().filter(|v| allowed.contains(v)).collect();
        matching.sort();
        if let Some(v) = matching.iter().rev().find(|v| v.is_stable()) {
            return Some(v.clone());
        }
        matching.into_iter().last()
    }

    pub fn solve(&mut self, root_deps: Vec<(String, Range)>) -> Result<Outcome, String> {
        let root_ver = self.root_version.clone();
        for (dep, r) in &root_deps {
            self.add_incompat(
                vec![
                    (ROOT.to_string(), Term::exact(root_ver.clone())),
                    (dep.clone(), Term::negative(r.clone())),
                ],
                Cause::Dependency {
                    package: ROOT.to_string(),
                    version: root_ver.to_string(),
                    dep: dep.clone(),
                },
            );
        }
        self.decide(ROOT, root_ver);
        let mut changed: Vec<String> = vec![ROOT.to_string()];

        let mut guard = 0usize;
        loop {
            guard += 1;
            if guard > 1_000_000 {
                return Err("resolver did not terminate (internal error)".into());
            }
            // ---- unit propagation ----
            while let Some(pkg) = changed.pop() {
                let ids = self.index.get(&pkg).cloned().unwrap_or_default();
                for ci in ids {
                    match self.eval(ci) {
                        Eval::Conflict => match self.conflict_resolution(ci) {
                            Conflict::NoSolution(id) => return Ok(Outcome::Unsolvable(id)),
                            Conflict::Learned(unit_pkg) => {
                                changed.clear();
                                changed.push(unit_pkg);
                                break;
                            }
                        },
                        Eval::Unit(qp, qt) => {
                            self.derive(&qp, qt.negate(), ci);
                            changed.push(qp);
                        }
                        Eval::Ignore => {}
                    }
                }
            }
            // ---- decision ----
            let mut candidates = self.required_undecided();
            if candidates.is_empty() {
                let mut packages = BTreeMap::new();
                for a in &self.assignments {
                    if a.decision && a.package != ROOT {
                        packages.insert(a.package.clone(), a.term.range.lower_version().unwrap().clone());
                    }
                }
                return Ok(Outcome::Solved(Resolution { packages }));
            }
            // Fail-fast heuristic: fewest satisfying versions first.
            candidates.sort_by_key(|p| self.choose_count(p));
            let pkg = candidates[0].clone();
            let allowed = self.term_for(&pkg);
            match self.choose_version(&pkg, &allowed) {
                None => {
                    self.add_incompat(
                        vec![(pkg.clone(), Term::positive(allowed))],
                        Cause::NoVersions { package: pkg.clone() },
                    );
                    changed.clear();
                    changed.push(pkg);
                }
                Some(ver) => {
                    let deps = self.provider.dependencies(&pkg, &ver).unwrap_or_default();
                    for (dep, r) in deps {
                        self.add_incompat(
                            vec![
                                (pkg.clone(), Term::exact(ver.clone())),
                                (dep.clone(), Term::negative(r)),
                            ],
                            Cause::Dependency {
                                package: pkg.clone(),
                                version: ver.to_string(),
                                dep,
                            },
                        );
                    }
                    self.decide(&pkg, ver);
                    changed.clear();
                    changed.push(pkg);
                }
            }
        }
    }

    fn choose_count(&self, pkg: &str) -> usize {
        let allowed = self.term_for(pkg);
        self.provider.versions(pkg).into_iter().filter(|v| allowed.contains(v)).count()
    }
}
