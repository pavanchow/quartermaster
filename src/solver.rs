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
        self.state_for(pkg, self.assignments.len()).allowed()
    }

    /// Accumulated per-package *state* for `pkg` over the first `upto`
    /// assignments: a signed term that, unlike a bare version range, remembers
    /// whether the package may still be absent.
    fn state_for(&self, pkg: &str, upto: usize) -> Term {
        let mut acc = Term::any_state();
        for a in &self.assignments[..upto] {
            if a.package == pkg {
                acc = acc.intersect(&a.term);
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
            let have = self.state_for(pkg, self.assignments.len());
            match term.relation_to(&have) {
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

    /// Smallest index `i < upto` at which `pkg`'s accumulated state first
    /// satisfies `term`, scanning only `pkg`'s assignments.
    fn satisfier_of(&self, pkg: &str, term: &Term, upto: usize) -> Option<usize> {
        let mut acc = Term::any_state();
        for i in 0..upto {
            if self.assignments[i].package == pkg {
                acc = acc.intersect(&self.assignments[i].term);
                if term.relation_to(&acc) == Relation::Satisfied {
                    return Some(i);
                }
            }
        }
        None
    }

    /// PubGrub conflict resolution. From a satisfied (conflicting)
    /// incompatibility, walk back by resolving it against the causes of its
    /// satisfiers until it becomes unit at an earlier level, then backjump
    /// there. If it reduces to the project's own requirements, there is no
    /// solution.
    fn conflict_resolution(&mut self, mut ci: IncompatId) -> Conflict {
        loop {
            if self.incompats[ci].is_terminal(ROOT) {
                return Conflict::NoSolution(ci);
            }
            let terms = self.incompats[ci].terms.clone();
            let n = self.assignments.len();

            // The most recent satisfier: the term whose satisfying assignment
            // comes latest. That assignment is what we resolve away.
            let mut sat_index = 0usize;
            let mut sat_pos = 0usize;
            for (k, (pkg, term)) in terms.iter().enumerate() {
                let i = self
                    .satisfier_of(pkg, term, n)
                    .expect("a satisfied incompatibility has a satisfier for every term");
                if i >= sat_index {
                    sat_index = i;
                    sat_pos = k;
                }
            }
            let p_pkg = terms[sat_pos].0.clone();
            let sat = &self.assignments[sat_index];
            let sat_level = sat.level;
            let sat_is_decision = sat.decision;
            let sat_cause = sat.cause;

            // Backjump target: the highest level at which every *other* term is
            // already satisfied, so the satisfier's term is the lone unit.
            let mut prev = 1usize;
            for (k, (pkg, term)) in terms.iter().enumerate() {
                if k == sat_pos {
                    continue;
                }
                let i = self.satisfier_of(pkg, term, n).unwrap();
                prev = prev.max(self.assignments[i].level);
            }

            if sat_is_decision || prev < sat_level {
                self.backtrack(prev);
                return Conflict::Learned(p_pkg);
            }

            let cause = sat_cause.expect("a derivation has a cause");
            ci = self.build_prior_cause(ci, &terms, sat_pos, cause);
        }
    }

    /// The prior cause of a resolution step: the conflicting incompatibility and
    /// the satisfier's cause merged (terms for a shared package intersected).
    /// The satisfier's package P is the resolution variable: its two terms are
    /// combined as their *union* (as sets of package states, absence included),
    /// and P is eliminated only when that union is universal. Dropping P whenever
    /// the version ranges merely cover every version would discard the fact that
    /// P must still be *present*, over-generalizing the learned clause and
    /// rejecting solvable instances (a package that could simply be left out).
    fn build_prior_cause(
        &mut self,
        ci: IncompatId,
        terms: &[(String, Term)],
        sat_pos: usize,
        cause: IncompatId,
    ) -> IncompatId {
        let p_pkg = terms[sat_pos].0.clone();
        let t1 = terms[sat_pos].1.clone();
        let t2 = self
            .incompats[cause]
            .term_for(&p_pkg)
            .cloned()
            .unwrap_or_else(|| Term::positive(Range::empty()));
        let shared = t1.union(&t2);
        let keep_shared = !shared.is_universal();

        let mut merged: Vec<(String, Term)> = Vec::new();
        let mut add = |q: &str, t: Term| {
            if let Some(slot) = merged.iter_mut().find(|(x, _)| x == q) {
                slot.1 = slot.1.intersect(&t);
            } else {
                merged.push((q.to_string(), t));
            }
        };
        for (k, (q, t)) in terms.iter().enumerate() {
            if k != sat_pos {
                add(q, t.clone());
            }
        }
        for (q, t) in &self.incompats[cause].terms {
            if *q != p_pkg {
                add(q, t.clone());
            }
        }
        if keep_shared {
            add(&p_pkg, shared);
        }
        self.add_incompat(merged, Cause::Derived(ci, cause))
    }

    /// Undecided packages we still have to pick a version for: those a positive
    /// requirement forces, plus the dependencies of already-decided packages.
    ///
    /// The second set matters for soundness. An earlier negative derivation (for
    /// example, learning a package has no version in some range) can leave a
    /// package constrained to a set that already *set-satisfies* a later
    /// dependency, so no positive term is ever derived for it. It still needs a
    /// concrete version. Its accumulated set is a subset of every dependency
    /// range that referenced it (that is exactly why those were set-satisfied),
    /// so any version chosen from that set satisfies them all.
    fn required_undecided(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for a in &self.assignments {
            if a.term.positive && a.package != ROOT && !seen.contains(&a.package) {
                seen.push(a.package.clone());
            }
        }
        for inc in &self.incompats {
            if let Cause::Dependency { package, version, dep } = &inc.cause {
                let decided_here =
                    self.decision(package).is_some_and(|v| v.to_string() == *version);
                if decided_here && !seen.contains(dep) {
                    seen.push(dep.clone());
                }
            }
        }
        seen.retain(|p| self.decision(p).is_none() && !self.term_for(p).is_empty());
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
