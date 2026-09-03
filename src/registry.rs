//! Where packages come from. The resolver only needs two questions answered:
//! which versions of a package exist, and what a given version depends on. That
//! is the [`Provider`] trait. [`Registry`] is a plain in-memory implementation
//! plus a readable text format so tests and the CLI can describe a package
//! universe without a server.
use crate::error::{Error, Result};
use crate::range::{self, Range};
use crate::version::Version;
use std::collections::BTreeMap;

/// One published package version and what it requires.
#[derive(Clone)]
pub struct Package {
    pub version: Version,
    pub deps: Vec<(String, Range)>,
}

pub trait Provider {
    /// All published versions of `name`, in any order (the resolver sorts).
    fn versions(&self, name: &str) -> Vec<Version>;
    /// The dependencies of a specific version, or `None` if it does not exist.
    fn dependencies(&self, name: &str, version: &Version) -> Option<Vec<(String, Range)>>;
}

#[derive(Default)]
pub struct Registry {
    packages: BTreeMap<String, Vec<Package>>,
}

impl Registry {
    pub fn new() -> Registry {
        Registry::default()
    }

    pub fn add(&mut self, name: &str, version: &str, deps: &[(&str, &str)]) -> Result<()> {
        let version = Version::parse(version)?;
        let mut parsed = Vec::new();
        for (dn, dc) in deps {
            parsed.push((dn.to_string(), range::parse(dc)?));
        }
        self.packages
            .entry(name.to_string())
            .or_default()
            .push(Package { version, deps: parsed });
        Ok(())
    }

    /// Parse the text registry format. One package version per stanza:
    /// ```text
    /// foo 1.2.0
    ///   bar ^1.0
    ///   baz >=2.0, <3.0
    /// foo 1.3.0
    ///   bar ^1.2
    /// ```
    /// A line with no leading whitespace starts a package version; indented
    /// lines are its dependencies (`name constraint`). Blank lines and `#`
    /// comments are ignored.
    pub fn parse(text: &str) -> Result<Registry> {
        // An in-progress package stanza: its name, version, and collected deps.
        type Stanza = (String, Version, Vec<(String, Range)>);
        let mut reg = Registry::new();
        let mut current: Option<Stanza> = None;
        let flush = |reg: &mut Registry, cur: Option<Stanza>| {
            if let Some((name, version, deps)) = cur {
                reg.packages.entry(name).or_default().push(Package { version, deps });
            }
        };
        for (lineno, raw) in text.lines().enumerate() {
            let line = match raw.split_once('#') {
                Some((code, _)) => code,
                None => raw,
            };
            if line.trim().is_empty() {
                continue;
            }
            let indented = line.starts_with(char::is_whitespace);
            let mut parts = line.split_whitespace();
            if indented {
                let dep = parts.next().ok_or_else(|| {
                    Error::Manifest(format!("line {}: empty dependency", lineno + 1))
                })?;
                let constraint: String = parts.collect::<Vec<_>>().join(" ");
                if constraint.is_empty() {
                    return Err(Error::Manifest(format!(
                        "line {}: dependency '{dep}' has no version constraint",
                        lineno + 1
                    )));
                }
                let r = range::parse(&constraint)?;
                match current.as_mut() {
                    Some((_, _, deps)) => deps.push((dep.to_string(), r)),
                    None => {
                        return Err(Error::Manifest(format!(
                            "line {}: dependency before any package",
                            lineno + 1
                        )))
                    }
                }
            } else {
                flush(&mut reg, current.take());
                let name = parts.next().unwrap();
                let ver = parts.next().ok_or_else(|| {
                    Error::Manifest(format!("line {}: package '{name}' has no version", lineno + 1))
                })?;
                current = Some((name.to_string(), Version::parse(ver)?, Vec::new()));
            }
        }
        flush(&mut reg, current.take());
        Ok(reg)
    }
}

impl Provider for Registry {
    fn versions(&self, name: &str) -> Vec<Version> {
        self.packages
            .get(name)
            .map(|v| v.iter().map(|p| p.version.clone()).collect())
            .unwrap_or_default()
    }
    fn dependencies(&self, name: &str, version: &Version) -> Option<Vec<(String, Range)>> {
        self.packages
            .get(name)?
            .iter()
            .find(|p| &p.version == version)
            .map(|p| p.deps.clone())
    }
}
