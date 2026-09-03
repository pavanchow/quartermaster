//! Turn a failed resolution into a proof a human can read. The terminal
//! incompatibility is the root of a derivation tree whose leaves are real facts
//! ("the project depends on X", "A 1.0 depends on B", "no version of C
//! matches ..."). A post-order walk renders that tree as a chain of "Because …
//! and …, …" lines ending in the contradiction. This is what most
//! build-your-own package managers cannot produce: not "resolution failed" but
//! the exact requirement path that made it impossible.
use crate::incompat::{Cause, IncompatId, Incompatibility};
use crate::solver::ROOT;
use crate::term::Term;

pub fn report(incompats: &[Incompatibility], terminal: IncompatId) -> String {
    let mut r = Reporter { incompats, lines: Vec::new() };
    r.visit(terminal);
    r.lines.push("So the project's dependencies cannot be satisfied.".to_string());
    number(&r.lines)
}

fn number(lines: &[String]) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{}. {}", i + 1, l))
        .collect::<Vec<_>>()
        .join("\n")
}

struct Reporter<'a> {
    incompats: &'a [Incompatibility],
    lines: Vec<String>,
}

impl<'a> Reporter<'a> {
    /// Returns a short phrase referring to this incompatibility's conclusion,
    /// emitting explanation lines for any derived causes along the way.
    fn visit(&mut self, id: IncompatId) -> String {
        match &self.incompats[id].cause {
            Cause::Derived(a, b) => {
                let pa = self.visit(*a);
                let pb = self.visit(*b);
                let concl = self.describe(id);
                self.lines.push(format!("Because {pa} and {pb}, {concl}."));
                concl
            }
            _ => self.describe(id),
        }
    }

    /// A one-clause description of what an incompatibility forbids, phrased as a
    /// requirement wherever the shape allows.
    fn describe(&self, id: IncompatId) -> String {
        let inc = &self.incompats[id];
        match &inc.cause {
            Cause::Dependency { package, version, dep } => {
                let r = dep_range(inc, dep);
                if package == ROOT {
                    format!("the project depends on {dep} {r}")
                } else {
                    format!("{package} {version} depends on {dep} {r}")
                }
            }
            Cause::NoVersions { package } => {
                let r = inc.term_for(package).map(range_of).unwrap_or_else(|| "*".into());
                format!("no version of {package} matches {r}")
            }
            Cause::Root => "the project is being installed".into(),
            Cause::Derived(..) => generic(inc),
        }
    }
}

/// The dependency range for `dep` in a dependency incompatibility. We stored it
/// as a negative term, so its `range` field is the required range directly.
fn dep_range(inc: &Incompatibility, dep: &str) -> String {
    inc.terms
        .iter()
        .find(|(p, _)| p == dep)
        .map(|(_, t)| t.range.friendly())
        .unwrap_or_else(|| "*".into())
}

fn range_of(t: &Term) -> String {
    t.allowed().friendly()
}

/// Fallback phrasing for a learned incompatibility, reading its remaining terms
/// as a joint requirement.
fn generic(inc: &Incompatibility) -> String {
    if inc.terms.len() == 1 && inc.terms[0].0 == ROOT {
        return "the project's requirements are contradictory".into();
    }
    let parts: Vec<String> = inc
        .terms
        .iter()
        .filter(|(p, _)| p != ROOT)
        .map(|(p, t)| {
            if t.positive {
                format!("{p} {}", t.range.friendly())
            } else {
                format!("{p} outside {}", t.range.friendly())
            }
        })
        .collect();
    match parts.len() {
        0 => "the requirements conflict".into(),
        1 => format!("{} cannot be used", parts[0]),
        _ => format!("{} cannot be chosen together", parts.join(" and ")),
    }
}
