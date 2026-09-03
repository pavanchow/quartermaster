//! `qm` command line. Reads a project manifest and a registry, resolves, and
//! either prints the pinned set / writes a lockfile / installs, or prints the
//! plain-English proof that the dependencies conflict.
use quartermaster::lock::Lockfile;
use quartermaster::manifest::Manifest;
use quartermaster::registry::Registry;
use quartermaster::{install, resolve, Resolved};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::exit;

const USAGE: &str = "\
qm - a package manager with a readable dependency resolver

USAGE:
    qm <command> [options]

COMMANDS:
    resolve  <manifest> --registry <file>        Resolve and print the pinned versions
    lock     <manifest> --registry <file> [-o F] Resolve and write a lockfile (default quartermaster.lock)
    install  <manifest> --registry <file> [--to D] Resolve and materialize into a directory (default qm_modules)
    tree     <manifest> --registry <file>        Print the resolved dependency tree
    explain  <manifest> --registry <file>        Explain a conflict, or confirm it resolves

A manifest lists the project's direct dependencies:
    name    myapp
    version 1.0.0
    require foo ^1.0
    require bar >=2.0, <3.0

A registry lists available package versions and their dependencies:
    foo 1.2.0
      bar ^1.0
    bar 1.5.0
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprint!("{USAGE}");
        exit(2);
    }
    let command = args[0].as_str();
    let rest = &args[1..];
    let result = match command {
        "resolve" => cmd_resolve(rest),
        "lock" => cmd_lock(rest),
        "install" => cmd_install(rest),
        "tree" => cmd_tree(rest),
        "explain" => cmd_explain(rest),
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            return;
        }
        other => {
            eprintln!("unknown command '{other}'\n\n{USAGE}");
            exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        exit(1);
    }
}

/// The flags that take a following value. Anything else starting with `-` is a
/// boolean or unknown flag and does NOT consume the next argument, so it cannot
/// swallow a positional (e.g. the manifest path).
const VALUE_FLAGS: &[&str] = &["--registry", "-r", "--out", "-o", "--to", "-t"];

/// Pull `--flag value`, `-short value`, or `--flag=value` out of the args.
fn take_opt(args: &[String], long: &str, short: &str) -> Option<String> {
    for (i, a) in args.iter().enumerate() {
        if a == long || a == short {
            return args.get(i + 1).cloned();
        }
        if let Some(v) = a.strip_prefix(&format!("{long}=")) {
            return Some(v.to_string());
        }
    }
    None
}

fn positional(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a.starts_with('-') {
            if VALUE_FLAGS.contains(&a.as_str()) {
                i += 2; // known value-taking flag consumes its value
            } else {
                i += 1; // boolean/unknown/`--key=value` flag consumes only itself
            }
        } else {
            out.push(a.clone());
            i += 1;
        }
    }
    out
}

fn load(args: &[String]) -> Result<(Manifest, Registry), String> {
    let manifest_path = positional(args)
        .into_iter()
        .next()
        .ok_or("a manifest path is required")?;
    let registry_path =
        take_opt(args, "--registry", "-r").ok_or("--registry <file> is required")?;
    let mtext = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("cannot read {manifest_path}: {e}"))?;
    let rtext = std::fs::read_to_string(&registry_path)
        .map_err(|e| format!("cannot read {registry_path}: {e}"))?;
    let manifest = Manifest::parse(&mtext).map_err(|e| e.to_string())?;
    let registry = Registry::parse(&rtext).map_err(|e| e.to_string())?;
    Ok((manifest, registry))
}

fn resolve_or_report(
    manifest: &Manifest,
    registry: &Registry,
) -> Result<BTreeMap<String, quartermaster::Version>, String> {
    match resolve(registry, manifest.deps.clone())? {
        Resolved::Ok(map) => Ok(map),
        Resolved::Conflict(why) => {
            eprintln!("cannot resolve {} {}:\n", manifest.name, manifest.version);
            eprintln!("{why}");
            exit(1);
        }
    }
}

fn cmd_resolve(args: &[String]) -> Result<(), String> {
    let (m, r) = load(args)?;
    let map = resolve_or_report(&m, &r)?;
    println!("resolved {} {} ({} packages):", m.name, m.version, map.len());
    for (name, version) in &map {
        println!("  {name} {version}");
    }
    Ok(())
}

fn cmd_lock(args: &[String]) -> Result<(), String> {
    let (m, r) = load(args)?;
    let map = resolve_or_report(&m, &r)?;
    let out = take_opt(args, "--out", "-o").unwrap_or_else(|| "quartermaster.lock".into());
    let text = Lockfile::new(map).to_text();
    std::fs::write(&out, &text).map_err(|e| format!("cannot write {out}: {e}"))?;
    println!("wrote {out}");
    Ok(())
}

fn cmd_install(args: &[String]) -> Result<(), String> {
    let (m, r) = load(args)?;
    let map = resolve_or_report(&m, &r)?;
    let to = take_opt(args, "--to", "-t").unwrap_or_else(|| "qm_modules".into());
    let n = install::install(Path::new(&to), &r, &map).map_err(|e| e.to_string())?;
    println!("installed {n} packages into {to}/");
    Ok(())
}

fn cmd_tree(args: &[String]) -> Result<(), String> {
    let (m, r) = load(args)?;
    let map = resolve_or_report(&m, &r)?;
    println!("{} {}", m.name, m.version);
    let mut direct: Vec<String> = m.deps.iter().map(|(d, _)| d.clone()).collect();
    direct.sort();
    let mut seen = Vec::new();
    for (i, dep) in direct.iter().enumerate() {
        print_tree(dep, &map, &r, "", i + 1 == direct.len(), &mut seen);
    }
    Ok(())
}

fn print_tree(
    name: &str,
    map: &BTreeMap<String, quartermaster::Version>,
    registry: &Registry,
    prefix: &str,
    last: bool,
    seen: &mut Vec<String>,
) {
    let branch = if last { "└─ " } else { "├─ " };
    let version = match map.get(name) {
        Some(v) => v.to_string(),
        None => "?".into(),
    };
    let repeat = seen.contains(&name.to_string());
    println!("{prefix}{branch}{name} {version}{}", if repeat { " (*)" } else { "" });
    if repeat {
        return;
    }
    seen.push(name.to_string());
    use quartermaster::registry::Provider;
    if let Some(ver) = map.get(name) {
        let mut deps = registry.dependencies(name, ver).unwrap_or_default();
        deps.sort_by(|a, b| a.0.cmp(&b.0));
        let child_prefix = format!("{prefix}{}", if last { "   " } else { "│  " });
        for (i, (dep, _)) in deps.iter().enumerate() {
            print_tree(dep, map, registry, &child_prefix, i + 1 == deps.len(), seen);
        }
    }
}

fn cmd_explain(args: &[String]) -> Result<(), String> {
    let (m, r) = load(args)?;
    match resolve(&r, m.deps.clone())? {
        Resolved::Ok(map) => {
            println!("{} {} resolves cleanly ({} packages).", m.name, m.version, map.len());
            Ok(())
        }
        Resolved::Conflict(why) => {
            println!("{why}");
            exit(1);
        }
    }
}
