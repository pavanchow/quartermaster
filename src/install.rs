//! Materialize a resolved set into a directory. Quartermaster has no network
//! and no tarballs, so "install" writes each resolved package's identity and
//! dependencies from the registry into `<out>/<name>/installed.qm`. It is the
//! honest minimum: the resolver decided the exact set, and this lays that set
//! down on disk deterministically.
use crate::error::Result;
use crate::registry::Provider;
use crate::version::Version;
use std::collections::BTreeMap;
use std::path::Path;

pub fn install(
    out: &Path,
    provider: &dyn Provider,
    resolution: &BTreeMap<String, Version>,
) -> Result<usize> {
    for (name, version) in resolution {
        let dir = out.join(name);
        std::fs::create_dir_all(&dir)?;
        let deps = provider.dependencies(name, version).unwrap_or_default();
        let mut body = format!("name {name}\nversion {version}\n");
        for (dep, range) in deps {
            body.push_str(&format!("require {dep} {}\n", range.friendly()));
        }
        std::fs::write(dir.join("installed.qm"), body)?;
    }
    Ok(resolution.len())
}
