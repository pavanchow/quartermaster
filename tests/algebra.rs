use quartermaster::range::{self, Range};
use quartermaster::Version;

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}
fn r(s: &str) -> Range {
    range::parse(s).unwrap()
}

#[test]
fn version_parse_and_order() {
    assert!(v("1.0.0") < v("1.0.1"));
    assert!(v("1.9.0") < v("1.10.0"));
    assert!(v("2.0.0") > v("1.99.99"));
    // a prerelease is lower than its release
    assert!(v("1.0.0-rc.1") < v("1.0.0"));
    assert!(v("1.0.0-alpha") < v("1.0.0-beta"));
    assert!(v("1.0.0-rc.1") < v("1.0.0-rc.2"));
    assert!(v("1.0.0-1") < v("1.0.0-alpha")); // numeric < alphanumeric
    assert!(!v("1.0.0-rc").is_stable());
    assert!(v("1.0.0").is_stable());
}

#[test]
fn version_rejects_garbage() {
    assert!(Version::parse("1.0").is_err());
    assert!(Version::parse("1.0.0.0").is_err()); // genuine over-specification
    assert!(Version::parse("x.0.0").is_err());
    assert!(Version::parse("1.0.0-").is_err());
}

#[test]
fn version_tolerates_a_trailing_dot() {
    // A stray trailing dot (`1.1.5.`) is an unambiguous typo, not an error.
    assert_eq!(Version::parse("1.1.5.").unwrap(), v("1.1.5"));
    assert_eq!(Version::parse("  2.0.0  ").unwrap(), v("2.0.0"));
    assert!(r("^1.2.").contains(&v("1.5.0"))); // and in constraints
}

#[test]
fn caret_ranges() {
    let c = r("^1.2.3");
    assert!(c.contains(&v("1.2.3")));
    assert!(c.contains(&v("1.9.0")));
    assert!(!c.contains(&v("1.2.2")));
    assert!(!c.contains(&v("2.0.0")));
    // caret on 0.x is narrower
    let z = r("^0.2.3");
    assert!(z.contains(&v("0.2.9")));
    assert!(!z.contains(&v("0.3.0")));
}

#[test]
fn tilde_and_comparators() {
    let t = r("~1.2.0");
    assert!(t.contains(&v("1.2.9")));
    assert!(!t.contains(&v("1.3.0")));
    let ge = r(">=1.5");
    assert!(ge.contains(&v("1.5.0")));
    assert!(!ge.contains(&v("1.4.9")));
    let band = r(">=1.0, <2.0");
    assert!(band.contains(&v("1.9.9")));
    assert!(!band.contains(&v("2.0.0")));
}

#[test]
fn union_and_disjunction() {
    let u = r("^1.0 || ^3.0");
    assert!(u.contains(&v("1.5.0")));
    assert!(!u.contains(&v("2.0.0")));
    assert!(u.contains(&v("3.1.0")));
}

#[test]
fn intersect_union_complement_are_consistent() {
    let a = r(">=1.0, <5.0");
    let b = r(">=3.0, <8.0");
    let inter = a.intersect(&b);
    assert!(inter.contains(&v("3.0.0")) && inter.contains(&v("4.9.0")));
    assert!(!inter.contains(&v("2.0.0")) && !inter.contains(&v("5.0.0")));

    // complement of complement is the original set (spot-checked by membership)
    let cc = a.complement().complement();
    for s in ["0.5.0", "1.0.0", "4.9.9", "5.0.0", "9.0.0"] {
        assert_eq!(cc.contains(&v(s)), a.contains(&v(s)), "mismatch at {s}");
    }
    // a set and its complement never share a member
    let comp = a.complement();
    for s in ["1.0.0", "3.0.0", "4.9.0", "0.0.1", "6.0.0"] {
        assert!(a.contains(&v(s)) != comp.contains(&v(s)), "overlap at {s}");
    }
}

#[test]
fn empty_intersection() {
    let a = r("^1.0");
    let b = r("^2.0");
    assert!(a.intersect(&b).is_empty());
}

#[test]
fn friendly_round_trips_common_shapes() {
    assert_eq!(r("^1.2.3").friendly(), "^1.2.3");
    assert_eq!(r("~1.2.0").friendly(), "~1.2.0");
    assert_eq!(r("=1.0.0").friendly(), "1.0.0");
}

#[test]
fn huge_version_part_errors_instead_of_panicking() {
    // A `+1` on the caret/tilde/x-range upper bound must not overflow u64.
    assert!(range::parse("^18446744073709551615").is_err());
    assert!(range::parse("~1.18446744073709551615").is_err());
    assert!(range::parse("18446744073709551615").is_err());
}
