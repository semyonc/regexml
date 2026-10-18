// Whole-string matching, `Regex::is_full_match`: the matching operation of
// XML Schema's `pattern` facet. XSD 1.1 Part 2 appendix G: "the regular
// expression language defined here implicitly anchors all regular
// expressions at the head and tail". The cases here add to those that came
// with the method (Paligo/regexml#15, in test_regexml.rs): the empty string,
// alternatives and repetitions that only match the whole string after
// backtracking, the program optimizations that run before a search
// (`is_match`) but not before a full match, and `^` and `$`, which are
// ordinary characters in the XSD dialect.

use regexml_fork::Regex;

#[track_caller]
fn full(regex: &Regex, cases: &[(&str, bool)]) {
    for (haystack, expected) in cases {
        assert_eq!(
            regex.is_full_match(haystack),
            *expected,
            "is_full_match({haystack:?})"
        );
    }
}

#[test]
fn the_empty_string() {
    full(&Regex::xsd("", "").unwrap(), &[("", true), ("a", false)]);
    full(&Regex::xsd("a*", "").unwrap(), &[("", true), ("aa", true)]);
    full(&Regex::xsd("a+", "").unwrap(), &[("", false), ("a", true)]);
    // an empty branch matches the empty string, and only it
    full(
        &Regex::xsd("a|", "").unwrap(),
        &[("", true), ("a", true), ("aa", false)],
    );
    full(&Regex::xsd("()", "").unwrap(), &[("", true), ("a", false)]);
}

#[test]
fn alternatives_are_backtracked_into() {
    // the first alternative that matches is not the one that reaches the end
    let regex = Regex::xsd("a|ab", "").unwrap();
    full(&regex, &[("ab", true), ("a", true), ("b", false)]);
    let regex = Regex::xsd("(a|ab)(c|bcd)", "").unwrap();
    full(
        &regex,
        &[("abcd", true), ("abc", true), ("ac", true), ("abd", false)],
    );
    // a later choice decides how an earlier repetition has to end
    let regex = Regex::xsd("(ab|a)*(b|bc)", "").unwrap();
    full(&regex, &[("abab", true), ("aabc", true), ("ababcb", false)]);
}

#[test]
fn repetitions_are_backtracked_into() {
    full(
        &Regex::xsd("a*ab", "").unwrap(),
        &[("aaab", true), ("ab", true), ("aab", true), ("b", false)],
    );
    full(
        &Regex::xsd("[ab]*b", "").unwrap(),
        &[("abab", true), ("aba", false)],
    );
    full(
        &Regex::xsd("a{2,3}a", "").unwrap(),
        &[
            ("aaa", true),
            ("aaaa", true),
            ("aa", false),
            ("aaaaa", false),
        ],
    );
    full(
        &Regex::xsd(r"(\d+)(\d{2})", "").unwrap(),
        &[("123", true), ("12", false)],
    );
}

#[test]
fn reluctant_repetitions_extend_to_the_end() {
    // XPath dialect only; a reluctant repetition still has to take the whole
    // string when nothing follows it
    full(
        &Regex::xpath("a+?", "").unwrap(),
        &[("aaa", true), ("", false), ("aab", false)],
    );
    full(
        &Regex::xpath("(a|b)*?c", "").unwrap(),
        &[("ababc", true), ("c", true), ("abab", false)],
    );
    full(&Regex::xpath("a{2,4}?", "").unwrap(), &[("aaaa", true)]);
}

#[test]
fn a_repetition_at_the_end_cannot_stop_early() {
    // `a*` and `\d+` at the end of the expression are compiled as
    // repetitions that do not give anything back; the end of the string
    // still has to be reached
    full(
        &Regex::xsd("a*", "").unwrap(),
        &[("aaa", true), ("aab", false), ("baa", false)],
    );
    full(
        &Regex::xsd(r"\d+", "").unwrap(),
        &[("123", true), ("123x", false), ("x123", false)],
    );
}

#[test]
fn search_optimizations_do_not_apply() {
    // a literal prefix, a first-character class and a minimum length make
    // `is_match` skip start positions; a full match starts at 0 only
    full(
        &Regex::xsd("abc", "").unwrap(),
        &[("abc", true), ("xabc", false), ("abcabc", false)],
    );
    full(
        &Regex::xsd("[xy]z", "").unwrap(),
        &[("xz", true), ("axz", false)],
    );
    full(
        &Regex::xsd("a{3}", "").unwrap(),
        &[("aaa", true), ("aaaa", false)],
    );
}

#[test]
fn caret_and_dollar_are_ordinary_characters_in_xsd() {
    // XSD 1.1 Part 2 §G.3: "A metacharacter is either '.', '\', '?', '*',
    // '+', '{', '}', '(', ')', '|', '[', or ']'." and "A normal character is
    // any XML character that is not a metacharacter."
    let regex = Regex::xsd("^a$", "").unwrap();
    full(&regex, &[("^a$", true), ("a", false)]);
    let regex = Regex::xsd("a^b", "").unwrap();
    full(&regex, &[("a^b", true), ("ab", false)]);
    // in the XPath dialect they are anchors (F&O 3.1 §5.6.1)
    let regex = Regex::xpath("^a$", "").unwrap();
    full(&regex, &[("a", true), ("^a$", false)]);
    let regex = Regex::xpath("a$\n^b", "m").unwrap();
    full(&regex, &[("a\nb", true), ("a\nbb", false)]);
}

#[test]
fn back_references_are_kept_while_matching() {
    // the only case where a full match has to keep its captures
    let regex = Regex::xpath(r"(a+)\1", "").unwrap();
    full(
        &regex,
        &[("aa", true), ("aaaa", true), ("aaa", false), ("", false)],
    );
    let regex = Regex::xpath(r"((a)|(ab))+c\2", "").unwrap();
    full(&regex, &[("aabca", true), ("aabc", false)]);
}

#[test]
fn characters_not_bytes_and_flags() {
    // the wildcard is one character, a supplementary one included
    let regex = Regex::xsd(".", "").unwrap();
    full(
        &regex,
        &[
            ("\u{1D11E}", true),
            ("é", true),
            ("ab", false),
            ("\n", false),
        ],
    );
    full(
        &Regex::xsd("abc", "i").unwrap(),
        &[("ABC", true), ("aBc", true), ("ABCD", false)],
    );
    full(
        &Regex::xsd(r"\p{IsNoSuchBlock}+", ";k").unwrap(),
        &[("anything", true), ("", false)],
    );
}

#[test]
fn a_regex_answers_more_than_once() {
    // the matcher is per call: one answer does not leak into the next
    let regex = Regex::xsd("(a|ab)(c|bcd)", "").unwrap();
    for _ in 0..3 {
        full(&regex, &[("abcd", true), ("abd", false), ("ac", true)]);
    }
}
