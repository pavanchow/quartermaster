//! The lockfile: the exact version chosen for every package, written in a
//! stable, sorted, line-per-package format so it diffs cleanly and a second
//! resolve with the same inputs reproduces it byte for byte.
//!
//! ```text
//! # quartermaster.lock
//! bar 2.1.0
//! foo 1.2.3
//! ```
use crate::error::{Error, Result};
use crate::version::Version;
use std::collections::BTreeMap;

pub struct Lockfile {
    pub packages: BTreeMap<String, Version>,
}

impl Lockfile {
    pub fn new(packages: BTreeMap<String, Version>) -> Lockfile {
        Lockfile { packages }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("# quartermaster.lock\n");
        for (name, version) in &self.packages {
            out.push_str(&format!("{name} {version}\n"));
        }
        out
    }

    pub fn parse(text: &str) -> Result<Lockfile> {
        let mut packages = BTreeMap::new();
        for (i, raw) in text.lines().enumerate() {
            let line = match raw.split_once('#') {
                Some((code, _)) => code,
                None => raw,
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (name, ver) = line.split_once(char::is_whitespace).ok_or_else(|| {
                Error::Lock(format!("line {}: expected 'name version'", i + 1))
            })?;
            packages.insert(name.trim().to_string(), Version::parse(ver.trim())?);
        }
        Ok(Lockfile { packages })
    }
}
