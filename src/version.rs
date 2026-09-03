//! Semantic versions: `major.minor.patch` with an optional dot-separated
//! prerelease tag. Ordering follows semver: numeric fields compare
//! numerically, a prerelease is lower than its release, and prerelease
//! identifiers compare per SemVer 2.0 (numeric < numeric numerically, numeric
//! before alphanumeric, alphanumeric lexically).
use crate::error::{Error, Result};
use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Prerelease identifiers, e.g. `1.0.0-rc.1` -> ["rc", "1"]. Empty for a
    /// normal release.
    pub pre: Vec<String>,
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Version {
        Version { major, minor, patch, pre: Vec::new() }
    }

    pub fn parse(s: &str) -> Result<Version> {
        let s = s.trim();
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        // Tolerate a trailing dot (`1.2.3.` is an unambiguous typo for `1.2.3`);
        // genuine over-specification like `1.2.3.4` still errors below.
        let core = core.trim_end_matches('.');
        let mut it = core.split('.');
        let mut num = |what: &str| -> Result<u64> {
            let part = it
                .next()
                .ok_or_else(|| Error::Version(format!("version '{s}' is missing the {what}")))?;
            part.parse::<u64>()
                .map_err(|_| Error::Version(format!("version '{s}' has a non-numeric {what}")))
        };
        let major = num("major")?;
        let minor = num("minor")?;
        let patch = num("patch")?;
        if it.next().is_some() {
            return Err(Error::Version(format!("version '{s}' has too many parts")));
        }
        let pre = match pre {
            None => Vec::new(),
            Some("") => {
                return Err(Error::Version(format!("version '{s}' has an empty prerelease")))
            }
            Some(p) => p.split('.').map(|x| x.to_string()).collect(),
        };
        Ok(Version { major, minor, patch, pre })
    }

    /// True for a normal release (no prerelease tag).
    pub fn is_stable(&self) -> bool {
        self.pre.is_empty()
    }
}

fn cmp_pre(a: &[String], b: &[String]) -> Ordering {
    // Per semver: a version with a prerelease is lower than one without.
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater, // no pre > has pre
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    for (x, y) in a.iter().zip(b.iter()) {
        let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(nx), Ok(ny)) => nx.cmp(&ny),
            (Ok(_), Err(_)) => Ordering::Less, // numeric < alphanumeric
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => x.cmp(y),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    a.len().cmp(&b.len())
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then_with(|| cmp_pre(&self.pre, &other.pre))
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}
impl fmt::Debug for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}
