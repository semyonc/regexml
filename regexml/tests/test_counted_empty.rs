// A counted repetition of an operand that can match the empty string, and a
// repetition of an operand that only matches the empty string.
//
// XSD 1.1 Part 2 §G.2: for a quantifier {n,m}, S{n,m} denotes "All strings in
// L(S)^k for all k such that n <= k <= m" — the upper bound counts however
// short the operand's strings are, and an empty operand string is still a
// repetition. F&O 3.1 §5.6.1 takes these quantifiers over unchanged. The
// expected values agree with Python's `re`.

use regexml_fork::Regex;

#[track_caller]
fn full(pattern: &str, cases: &[(&str, bool)]) {
    for dialect in ["xsd", "xpath"] {
        let regex = match dialect {
            "xsd" => Regex::xsd(pattern, ""),
            _ => Regex::xpath(pattern, ""),
        }
        .unwrap();
        for (haystack, expected) in cases {
            assert_eq!(
                regex.is_full_match(haystack),
                *expected,
                "{dialect} {pattern:?} on {haystack:?}"
            );
            let anchored = Regex::xpath(&format!("^(?:{pattern})$"), "").unwrap();
            assert_eq!(
                anchored.is_match(haystack),
                *expected,
                "^(?:{pattern})$ on {haystack:?}"
            );
        }
    }
}

#[track_caller]
fn search(pattern: &str, flags: &str, haystack: &str, expected: bool) {
    let regex = Regex::xpath(pattern, flags).unwrap();
    assert_eq!(
        regex.is_match(haystack),
        expected,
        "{pattern:?} on {haystack:?}"
    );
}

#[test]
fn an_upper_bound_counts_empty_iterations_too() {
    // the probe: `(a|){2}` is at most two a's
    full(
        "(a|){2}",
        &[
            ("", true),
            ("a", true),
            ("aa", true),
            ("aaa", false),
            ("aaaa", false),
        ],
    );
    full("(a?){3}", &[("", true), ("aaa", true), ("aaaa", false)]);
    full(
        "(a|b?){1,2}",
        &[("", true), ("b", true), ("ab", true), ("aba", false)],
    );
    full(
        "((a|)b?){2}",
        &[
            ("bb", true),
            ("abab", true),
            ("bbb", false),
            ("ababa", false),
        ],
    );
    full("(a|){1}b", &[("b", true), ("ab", true), ("aab", false)]);
    full("(a|){0}", &[("", true), ("a", false)]);
    // without an upper bound there is nothing to lose
    full("(a|){2,}", &[("", true), ("aaaa", true)]);
}

#[test]
fn the_xpath_forms_keep_the_bound_too() {
    let regex = Regex::xpath("^(?:a|){0,2}$", "").unwrap();
    assert!(regex.is_match("aa"));
    assert!(!regex.is_match("aaa"));
    let regex = Regex::xpath("^(a|){2}?$", "").unwrap();
    assert!(regex.is_match("aa"));
    assert!(!regex.is_match("aaa"));
}

#[test]
fn an_operand_that_only_matches_empty_is_kept_when_required() {
    // `($)+` is `$` once, not nothing
    search("^a($)+b", "", "ab", false);
    search("a($)+", "", "ab", false);
    search("^a($){2}$", "", "a", true);
    search("a(^)+", "", "ba", false);
    search("^(^)+a", "", "a", true);
    search("a\n(^)+b", "m", "a\nb", true);
    // when it may occur zero times, it may be left out
    search("a($)*b", "", "ab", true);
    search("a($)?b", "", "ab", true);
    search("a(?:$){1,3}", "", "xa", true);
}

#[test]
fn a_reluctant_quantifier_on_an_anchor() {
    // F&O 3.1 §5.6.1.1 makes `^` and `$` character classes, so they take
    // quantifiers, reluctant ones included; the `?` that makes a quantifier
    // reluctant was left over, and read as a quantifier with nothing before
    // it, when the quantified anchor allowed zero occurrences
    search("(?:$){0,}?a+?", "", "aa", true);
    search("a$??", "", "ab", true);
    search("^??a", "", "ba", true);
    search("a$*?", "", "ab", true);
    search("a(?:$)+?", "", "a", true);
    search("a(?:$)+?", "", "ab", false);
    search("a(?:$){2}?b", "", "ab", false);
    search("a${1,2}?", "", "ab", false);
}
