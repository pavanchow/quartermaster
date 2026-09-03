//! A `Range` is a set of versions: a union of disjoint version intervals. It is
//! the algebra the resolver reasons over. Every constraint string (`^1.2`,
//! `>=1.0, <2.0`, `1 || 2`, `*`) parses into one, and the resolver only ever
//! needs `contains`, `intersect`, `union`, `complement`, and `is_empty`.
//!
//! Intervals are stored as pairs of *cuts*. A cut is a position between
//! versions, so an inclusive and an exclusive bound at the same version are
//! distinct points and all four interval kinds ( [a,b], [a,b), (a,b], (a,b) )
//! have an exact, comparable representation. That makes the set operations
//! total and boundary-exact instead of a pile of special cases.
use crate::error::{Error, Result};
use crate::version::Version;
use std::cmp::Ordering;
use std::fmt;

/// A position on the version line. `Fin(v, 0)` is the cut immediately *before*
/// `v`; `Fin(v, 1)` is immediately *after* `v`. So an interval whose lower cut
/// is `Fin(v,0)` includes `v`, and one whose upper cut is `Fin(v,0)` excludes
/// it. `NegInf`/`PosInf` are the ends of the line.
#[derive(Clone, PartialEq, Eq)]
enum Cut {
    NegInf,
    Fin(Version, u8),
    PosInf,
}

impl Ord for Cut {
    fn cmp(&self, other: &Self) -> Ordering {
        use Cut::*;
        match (self, other) {
            (NegInf, NegInf) | (PosInf, PosInf) => Ordering::Equal,
            (NegInf, _) | (_, PosInf) => Ordering::Less,
            (_, NegInf) | (PosInf, _) => Ordering::Greater,
            (Fin(a, ea), Fin(b, eb)) => a.cmp(b).then(ea.cmp(eb)),
        }
    }
}
impl PartialOrd for Cut {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The lower and upper cut of a version `v` itself, used by `contains`.
fn before(v: &Version) -> Cut {
    Cut::Fin(v.clone(), 0)
}
fn after(v: &Version) -> Cut {
    Cut::Fin(v.clone(), 1)
}

#[derive(Clone, PartialEq, Eq)]
pub struct Range {
    /// Sorted, disjoint, non-touching intervals as (lower cut, upper cut).
    segs: Vec<(Cut, Cut)>,
}

impl Range {
    pub fn empty() -> Range {
        Range { segs: Vec::new() }
    }
    pub fn any() -> Range {
        Range { segs: vec![(Cut::NegInf, Cut::PosInf)] }
    }
    pub fn exact(v: Version) -> Range {
        Range { segs: vec![(before(&v), after(&v))] }
    }
    /// `>= v`
    pub fn at_least(v: Version) -> Range {
        Range { segs: vec![(before(&v), Cut::PosInf)] }
    }
    /// `> v`
    pub fn greater(v: Version) -> Range {
        Range { segs: vec![(after(&v), Cut::PosInf)] }
    }
    /// `<= v`
    pub fn at_most(v: Version) -> Range {
        Range { segs: vec![(Cut::NegInf, after(&v))] }
    }
    /// `< v`
    pub fn less(v: Version) -> Range {
        Range { segs: vec![(Cut::NegInf, before(&v))] }
    }
    /// `>= low, < high`
    pub fn between(low: Version, high: Version) -> Range {
        let (lo, hi) = (before(&low), before(&high));
        if lo < hi {
            Range { segs: vec![(lo, hi)] }
        } else {
            Range::empty()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }

    pub fn contains(&self, v: &Version) -> bool {
        let (b, a) = (before(v), after(v));
        self.segs.iter().any(|(lo, hi)| *lo <= b && a <= *hi)
    }

    pub fn complement(&self) -> Range {
        let mut out = Vec::new();
        let mut cursor = Cut::NegInf;
        for (lo, hi) in &self.segs {
            if cursor < *lo {
                out.push((cursor.clone(), lo.clone()));
            }
            cursor = hi.clone();
        }
        if cursor < Cut::PosInf {
            out.push((cursor, Cut::PosInf));
        }
        Range { segs: out }
    }

    pub fn union(&self, other: &Range) -> Range {
        let mut all: Vec<(Cut, Cut)> =
            self.segs.iter().cloned().chain(other.segs.iter().cloned()).collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out: Vec<(Cut, Cut)> = Vec::new();
        for (lo, hi) in all {
            match out.last_mut() {
                // Merge when the new segment touches or overlaps the last one.
                Some(last) if lo <= last.1 => {
                    if hi > last.1 {
                        last.1 = hi;
                    }
                }
                _ => out.push((lo, hi)),
            }
        }
        Range { segs: out }
    }

    pub fn intersect(&self, other: &Range) -> Range {
        let mut out = Vec::new();
        let (mut i, mut j) = (0, 0);
        while i < self.segs.len() && j < other.segs.len() {
            let (al, ah) = &self.segs[i];
            let (bl, bh) = &other.segs[j];
            let lo = al.max(bl).clone();
            let hi = ah.min(bh).clone();
            if lo < hi {
                out.push((lo, hi));
            }
            if ah <= bh {
                i += 1;
            } else {
                j += 1;
            }
        }
        Range { segs: out }
    }

    /// A human-friendly rendering that recognises caret and tilde shapes, so a
    /// resolved `[>=1.2.0, <2.0.0)` prints as `^1.2.0` and an exact point as the
    /// bare version. Falls back to the precise `Display` form for anything else.
    pub fn friendly(&self) -> String {
        if self.segs.len() != 1 {
            return self.to_string();
        }
        let (lo, hi) = &self.segs[0];
        // Exact point: [before(v), after(v)).
        if let (Cut::Fin(a, 0), Cut::Fin(b, 1)) = (lo, hi) {
            if a == b {
                return a.to_string();
            }
        }
        // Half-open [>=a, <b): recognise caret and tilde upper bounds.
        if let (Cut::Fin(a, 0), Cut::Fin(b, 0)) = (lo, hi) {
            if a.is_stable() && b.is_stable() {
                let caret = if a.major > 0 {
                    Version::new(a.major + 1, 0, 0)
                } else if a.minor > 0 {
                    Version::new(0, a.minor + 1, 0)
                } else {
                    Version::new(0, 0, a.patch + 1)
                };
                if *b == caret {
                    return format!("^{a}");
                }
                if *b == Version::new(a.major, a.minor + 1, 0) {
                    return format!("~{a}");
                }
            }
        }
        self.to_string()
    }

    /// The lowest and highest concrete version the range could hold, when the
    /// boundary is a real version (used by the version-selection heuristic to
    /// scan candidates). Not every range has finite ends.
    pub fn lower_version(&self) -> Option<&Version> {
        match self.segs.first() {
            Some((Cut::Fin(v, _), _)) => Some(v),
            _ => None,
        }
    }
}

/// Parse a caret/tilde/comparator/x-range constraint into a `Range`.
/// Grammar (loosely npm-flavoured, kept small and readable):
///   union       := intersection ( "||" intersection )*
///   intersection:= comparator ( ("," | whitespace) comparator )*
///   comparator  := ("^"|"~"|">="|">"|"<="|"<"|"=")? partial-version | "*"
pub fn parse(input: &str) -> Result<Range> {
    let input = input.trim();
    if input.is_empty() {
        return Err(Error::Range("empty constraint".into()));
    }
    let mut acc: Option<Range> = None;
    for alt in input.split("||") {
        let r = parse_intersection(alt.trim())?;
        acc = Some(match acc {
            None => r,
            Some(a) => a.union(&r),
        });
    }
    Ok(acc.unwrap())
}

fn parse_intersection(s: &str) -> Result<Range> {
    // Comparators are separated by commas or whitespace. A caret/tilde/exact is
    // a whole comparator on its own; `>= 1.0` may have a space after the op.
    let normalized = s.replace(',', " ");
    let toks: Vec<&str> = normalized.split_whitespace().collect();
    if toks.is_empty() {
        return Err(Error::Range(format!("empty constraint segment in '{s}'")));
    }
    // Re-glue a lone operator token onto the following version token.
    let mut comparators: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i];
        if matches!(t, ">=" | ">" | "<=" | "<" | "=") {
            let v = toks
                .get(i + 1)
                .ok_or_else(|| Error::Range(format!("operator '{t}' with no version")))?;
            comparators.push(format!("{t}{v}"));
            i += 2;
        } else {
            comparators.push(t.to_string());
            i += 1;
        }
    }
    let mut acc = Range::any();
    for c in comparators {
        acc = acc.intersect(&parse_comparator(&c)?);
    }
    Ok(acc)
}

/// A partial version: major with optional minor and patch, no prerelease.
struct Partial {
    major: u64,
    minor: Option<u64>,
    patch: Option<u64>,
}

fn parse_partial(s: &str) -> Result<Partial> {
    let mut it = s.split('.');
    let num = |o: Option<&str>, what: &str| -> Result<Option<u64>> {
        match o {
            None => Ok(None),
            Some(p) => p
                .parse::<u64>()
                .map(Some)
                .map_err(|_| Error::Range(format!("'{s}' has a non-numeric {what}"))),
        }
    };
    let major = num(it.next(), "major")?
        .ok_or_else(|| Error::Range(format!("'{s}' has no major version")))?;
    let minor = num(it.next(), "minor")?;
    let patch = num(it.next(), "patch")?;
    if it.next().is_some() {
        return Err(Error::Range(format!("'{s}' has too many version parts")));
    }
    Ok(Partial { major, minor, patch })
}

/// Parse a version that may omit minor/patch, filling the missing parts with 0.
/// So `1` is `1.0.0` and `1.2` is `1.2.0`. Used by the comparison operators,
/// where `>=1.0` should mean `>=1.0.0`.
fn loose(s: &str) -> Result<Version> {
    let p = parse_partial(s)?;
    Ok(Version::new(p.major, p.minor.unwrap_or(0), p.patch.unwrap_or(0)))
}

fn parse_comparator(c: &str) -> Result<Range> {
    if c == "*" || c == "x" || c == "X" {
        // Every real version, but bounded below so that depending on `*` still
        // forces the package to be selected (an unbounded "any" term is vacuous
        // in the resolver and would not require the dependency at all).
        return Ok(Range::at_least(Version::new(0, 0, 0)));
    }
    if let Some(rest) = c.strip_prefix(">=") {
        return Ok(Range::at_least(loose(rest)?));
    }
    if let Some(rest) = c.strip_prefix("<=") {
        return Ok(Range::at_most(loose(rest)?));
    }
    if let Some(rest) = c.strip_prefix('>') {
        return Ok(Range::greater(loose(rest)?));
    }
    if let Some(rest) = c.strip_prefix('<') {
        return Ok(Range::less(loose(rest)?));
    }
    if let Some(rest) = c.strip_prefix('=') {
        return Ok(Range::exact(loose(rest)?));
    }
    if let Some(rest) = c.strip_prefix('^') {
        return caret(&parse_partial(rest)?);
    }
    if let Some(rest) = c.strip_prefix('~') {
        return tilde(&parse_partial(rest)?);
    }
    // A bare partial version is an x-range: `1.2` -> >=1.2.0 <1.3.0, `1` -> ^...
    x_range(&parse_partial(c)?)
}

fn caret(p: &Partial) -> Result<Range> {
    let low = Version::new(p.major, p.minor.unwrap_or(0), p.patch.unwrap_or(0));
    // Up to (but excluding) the next version that changes the leftmost nonzero.
    let high = if p.major > 0 {
        Version::new(p.major + 1, 0, 0)
    } else if p.minor.unwrap_or(0) > 0 {
        Version::new(0, p.minor.unwrap() + 1, 0)
    } else {
        Version::new(0, 0, p.patch.unwrap_or(0) + 1)
    };
    Ok(Range::between(low, high))
}

fn tilde(p: &Partial) -> Result<Range> {
    let low = Version::new(p.major, p.minor.unwrap_or(0), p.patch.unwrap_or(0));
    // `~1.2.3` and `~1.2` both allow patch-level changes; `~1` allows minor.
    let high = match p.minor {
        Some(m) => Version::new(p.major, m + 1, 0),
        None => Version::new(p.major + 1, 0, 0),
    };
    Ok(Range::between(low, high))
}

fn x_range(p: &Partial) -> Result<Range> {
    match (p.minor, p.patch) {
        (Some(m), Some(pt)) => Ok(Range::exact(Version::new(p.major, m, pt))),
        (Some(m), None) => Ok(Range::between(
            Version::new(p.major, m, 0),
            Version::new(p.major, m + 1, 0),
        )),
        (None, _) => Ok(Range::between(
            Version::new(p.major, 0, 0),
            Version::new(p.major + 1, 0, 0),
        )),
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segs.is_empty() {
            return write!(f, "<nothing>");
        }
        if self.segs.len() == 1 && self.segs[0].0 == Cut::NegInf && self.segs[0].1 == Cut::PosInf {
            return write!(f, "*");
        }
        let parts: Vec<String> = self.segs.iter().map(|(lo, hi)| seg_to_string(lo, hi)).collect();
        write!(f, "{}", parts.join(" || "))
    }
}

fn seg_to_string(lo: &Cut, hi: &Cut) -> String {
    // Render an exact point compactly.
    if let (Cut::Fin(a, 0), Cut::Fin(b, 1)) = (lo, hi) {
        if a == b {
            return format!("={a}");
        }
    }
    let l = match lo {
        Cut::NegInf => None,
        Cut::Fin(v, 0) => Some(format!(">={v}")),
        Cut::Fin(v, _) => Some(format!(">{v}")),
        Cut::PosInf => Some("<nothing>".into()),
    };
    let h = match hi {
        Cut::PosInf => None,
        Cut::Fin(v, 1) => Some(format!("<={v}")),
        Cut::Fin(v, _) => Some(format!("<{v}")),
        Cut::NegInf => Some("<nothing>".into()),
    };
    match (l, h) {
        (Some(l), Some(h)) => format!("{l}, {h}"),
        (Some(l), None) => l,
        (None, Some(h)) => h,
        (None, None) => "*".into(),
    }
}

impl fmt::Debug for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}
