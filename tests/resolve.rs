use quartermaster::{range, registry::Registry, resolve, Resolved};

fn reg(text: &str) -> Registry {
    Registry::parse(text).expect("registry parses")
}

fn deps(pairs: &[(&str, &str)]) -> Vec<(String, quartermaster::Range)> {
    pairs
        .iter()
        .map(|(n, c)| (n.to_string(), range::parse(c).unwrap()))
        .collect()
}

fn solved(r: &Registry, top: &[(&str, &str)]) -> Vec<(String, String)> {
    match resolve(r, deps(top)).unwrap() {
        Resolved::Ok(map) => map.into_iter().map(|(k, v)| (k, v.to_string())).collect(),
        Resolved::Conflict(why) => panic!("expected a solution, got conflict:\n{why}"),
    }
}

fn conflict(r: &Registry, top: &[(&str, &str)]) -> String {
    match resolve(r, deps(top)).unwrap() {
        Resolved::Ok(map) => panic!("expected a conflict, got solution: {map:?}"),
        Resolved::Conflict(why) => why,
    }
}

#[test]
fn single_package() {
    let r = reg("foo 1.0.0\nfoo 1.1.0\nfoo 2.0.0");
    assert_eq!(solved(&r, &[("foo", "^1.0")]), vec![("foo".into(), "1.1.0".into())]);
}

#[test]
fn picks_highest_in_range() {
    let r = reg("foo 1.0.0\nfoo 1.4.2\nfoo 1.9.0\nfoo 2.0.0\nfoo 2.1.0");
    assert_eq!(solved(&r, &[("foo", ">=1.0, <2.0")]), vec![("foo".into(), "1.9.0".into())]);
}

#[test]
fn transitive() {
    let r = reg(
        "app 1.0.0\n  foo ^1.0\nfoo 1.2.0\n  bar ^1.0\nbar 1.5.0\nbar 1.6.0",
    );
    let s = solved(&r, &[("app", "^1.0")]);
    assert!(s.contains(&("app".into(), "1.0.0".into())));
    assert!(s.contains(&("foo".into(), "1.2.0".into())));
    assert!(s.contains(&("bar".into(), "1.6.0".into())));
}

#[test]
fn backtracks_over_a_dead_high_version() {
    // a 2.0.0 needs b ^2 which does not exist; the resolver must abandon the
    // highest a and settle on a 1.0.0, which needs b ^1.
    let r = reg(
        "a 1.0.0\n  b ^1.0\na 2.0.0\n  b ^2.0\nb 1.0.0\nb 1.1.0",
    );
    let s = solved(&r, &[("a", "*")]);
    assert!(s.contains(&("a".into(), "1.0.0".into())), "got {s:?}");
    assert!(s.contains(&("b".into(), "1.1.0".into())), "got {s:?}");
}

#[test]
fn deeper_backtracking() {
    // Choosing the newest of everything fails; the resolver must walk back.
    let r = reg(
        "root 1.0.0\n  a ^1.0\n  b ^1.0\n\
         a 1.0.0\n  shared >=2.0\n\
         a 1.1.0\n  shared >=3.0\n\
         b 1.0.0\n  shared <3.0\n\
         b 1.1.0\n  shared <2.0\n\
         shared 2.0.0\nshared 2.5.0\nshared 3.0.0",
    );
    // b 1.1.0 needs shared <2 (impossible with a's >=2), b 1.0.0 needs shared <3,
    // a 1.1.0 needs shared >=3 (conflicts with b<3), so a 1.0.0 + b 1.0.0 + shared 2.5.0.
    let s = solved(&r, &[("root", "^1.0")]);
    assert!(s.contains(&("a".into(), "1.0.0".into())), "got {s:?}");
    assert!(s.contains(&("b".into(), "1.0.0".into())), "got {s:?}");
    assert!(s.contains(&("shared".into(), "2.5.0".into())), "got {s:?}");
}

#[test]
fn no_matching_version_is_a_conflict() {
    let r = reg("foo 1.0.0\nfoo 1.5.0");
    let why = conflict(&r, &[("foo", "^2.0")]);
    assert!(why.contains("foo"), "explanation should name foo:\n{why}");
    assert!(why.to_lowercase().contains("no version") || why.contains("depends on foo"), "{why}");
}

#[test]
fn diamond_conflict_explains_both_sides() {
    // foo needs shared ^1, bar needs shared ^2, both required: unsatisfiable.
    let r = reg(
        "foo 1.0.0\n  shared ^1.0\n\
         bar 1.0.0\n  shared ^2.0\n\
         shared 1.0.0\nshared 2.0.0",
    );
    let why = conflict(&r, &[("foo", "^1.0"), ("bar", "^1.0")]);
    assert!(why.contains("foo") && why.contains("bar"), "should name both:\n{why}");
    assert!(why.contains("shared"), "should name shared:\n{why}");
    // The proof should reach a final contradiction line.
    assert!(why.contains("cannot be satisfied"), "should conclude:\n{why}");
}

#[test]
fn prefers_stable_over_prerelease() {
    let r = reg("foo 1.0.0\nfoo 2.0.0-rc.1");
    // ^1 excludes 2.0.0-rc.1 anyway; use * to prove the preference.
    assert_eq!(solved(&r, &[("foo", "*")]), vec![("foo".into(), "1.0.0".into())]);
}

// ---- soundness regressions (found by differential fuzzing vs a SAT oracle) ----

#[test]
fn negative_derivation_does_not_drop_a_required_dependency() {
    // `b 1.0.0` needs `c >=1.1` (none exists), forcing `b 0.1.0`, whose own
    // dependency `c <2.0` was already set-satisfied by the learned "c has no
    // version >=1.1". `c` must still be selected, not silently dropped.
    let r = reg("b 1.0.0\n  c >=1.1\nb 0.1.0\n  c <2.0\nc 1.0.0");
    let s = solved(&r, &[("b", "<2.0")]);
    assert!(s.contains(&("b".into(), "0.1.0".into())), "got {s:?}");
    assert!(s.contains(&("c".into(), "1.0.0".into())), "c must be resolved, got {s:?}");
}

#[test]
fn does_not_falsely_report_unsat_when_an_alternate_branch_solves() {
    // The `a 1.2.0` / `a 1.0.0` branches lead to a dead `d` subtree; the resolver
    // must fall back to `a 0.2.0 -> c 1.0.0`, not wrongly prove no solution.
    let r = reg(
        "a 1.2.0\n  d <2.0\na 1.0.0\n  d 1.0.0\n  b ^0.1\na 0.2.0\n  c ^1.0\n\
         b 2.0.0\n  d 1.0.0\nb 1.0.0\nc 1.0.0\nc 1.2.0\n  d >=1.1\n\
         c 1.1.0\n  a ^1.0\n  a ^1.1\nd 1.0.0\n  a ^0.1\nd 2.0.0\n  b ^1.1",
    );
    let s = solved(&r, &[("a", "<2.0")]);
    assert!(s.contains(&("a".into(), "0.2.0".into())), "got {s:?}");
    assert!(s.contains(&("c".into(), "1.0.0".into())), "got {s:?}");
}

#[test]
fn one_package_two_constraints_from_one_version() {
    // A single package version may list the same dependency twice; the resolver
    // must intersect both constraints.
    let r = reg("app 1.0.0\n  lib ^1.0\n  lib >=1.2\nlib 1.1.0\nlib 1.3.0");
    assert!(solved(&r, &[("app", "^1.0")]).contains(&("lib".into(), "1.3.0".into())));
}
