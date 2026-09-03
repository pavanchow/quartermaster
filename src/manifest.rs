//! The project manifest: the file that names a project and lists its direct
//! dependencies. Deliberately a tiny line format, not TOML, so the parser is a
//! few lines you can read:
//!
//! ```text
//! name    myapp
//! version 1.0.0
//! require foo ^1.0
//! require bar >=2.0, <3.0
//! ```
use crate::error::{Error, Result};
use crate::range::{self, Range};
use crate::version::Version;

pub struct Manifest {
    pub name: String,
    pub version: Version,
    pub deps: Vec<(String, Range)>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest> {
        let mut name = None;
        let mut version = None;
        let mut deps = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = match raw.split_once('#') {
                Some((code, _)) => code,
                None => raw,
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (key, rest) = line.split_once(char::is_whitespace).ok_or_else(|| {
                Error::Manifest(format!("line {}: expected 'key value'", i + 1))
            })?;
            let rest = rest.trim();
            match key {
                "name" => name = Some(rest.to_string()),
                "version" => version = Some(Version::parse(rest)?),
                "require" => {
                    let (dep, constraint) = rest.split_once(char::is_whitespace).ok_or_else(|| {
                        Error::Manifest(format!("line {}: 'require' needs a name and a constraint", i + 1))
                    })?;
                    deps.push((dep.trim().to_string(), range::parse(constraint.trim())?));
                }
                other => {
                    return Err(Error::Manifest(format!("line {}: unknown key '{other}'", i + 1)))
                }
            }
        }
        Ok(Manifest {
            name: name.ok_or_else(|| Error::Manifest("missing 'name'".into()))?,
            version: version.ok_or_else(|| Error::Manifest("missing 'version'".into()))?,
            deps,
        })
    }
}
