use icu_collections::codepointinvlist::CodePointInversionListBuilder;
use icu_properties::props::{GeneralCategory, GeneralCategoryGroup};
use icu_properties::CodePointMapData;
use regexml_fork::{Error, Regex};

#[test]
fn test_infinite_loop() {
    // {0,0} is really a no-op, but could lead to an infinite loop
    // in the FixedGreedy operation. Now we create a noop in such a situation.
    let regex = Regex::xpath(r#"(( a | ( bc ) ) {0,0} )+ xyz"#, "x");
    let regex = regex.unwrap();
    assert!(regex.is_match(r#"xyz"#));
}

#[test]
fn test_l_category_group() {
    let c = '㐀';

    let set = CodePointMapData::<GeneralCategory>::new()
        .get_set_for_value_group(GeneralCategoryGroup::Letter);
    let inv_list = set.to_code_point_inversion_list();
    let mut builder = CodePointInversionListBuilder::new();
    builder.add_set(&inv_list);
    let b = builder.build();
    assert!(b.contains(c));
}

#[test]
fn test_l_category_membership() {
    let regex = Regex::xpath(r#"^\p{L}"#, "");
    let regex = regex.unwrap();
    assert!(regex.is_match(r#"㐀"#));
}

#[test]
fn test_re00036_match_failure() {
    let matches_regex = Regex::xpath(r"^(?:(?:foo)?|bar)*$", "").unwrap();

    assert!(matches_regex.is_match("barfoobar"));
}

#[test]
fn test_low_surrogates() {
    let matches_regex = Regex::xpath(r"^(?:\p{IsLowSurrogates}?)$", "").unwrap();

    assert!(matches_regex.is_match(""));
}

#[test]
fn test_syntax() {
    let err = Regex::xpath(r"[^-[bc]]$", "").unwrap_err();
    assert_eq!(
        err,
        Error::Syntax("Nothing before subtraction operator".to_string())
    )
}

// Under `i`, a repetition followed by a literal whose first character has
// no case variants (a digit, a punctuation mark), or whose ICU case closure
// is disjoint from the repeated class, was made possessive: it kept every
// character it could, the literal's included, and the expression matched
// nothing. In 0.3.0 each of these was false for `is_match` and
// `is_full_match`, and `replace_all` left the input unchanged.
const CASE_BLIND_REPETITION_BEFORE_A_LITERAL: [(&str, &str); 10] = [
    (r".*\.xml", "file.XML"),
    (r".*\.xml", "file.xml"),
    (".*1", "a1"),
    (r"\d*5", "55"),
    (r"[a-z\-]*-1", "a-1"),
    (r"\S+@\S+", "a@b"),
    (".+,.+", "a,b"),
    (".*/", "a/b/"),
    (r"\s*\(.*\)", " (x)"),
    ("a*A", "aA"),
];

#[test]
fn test_case_blind_repetition_before_a_literal() {
    for (pattern, haystack) in CASE_BLIND_REPETITION_BEFORE_A_LITERAL {
        let regex = Regex::xpath(pattern, "i").unwrap();
        assert!(regex.is_match(haystack), "{pattern:?} on {haystack:?}");
        assert!(regex.is_full_match(haystack), "{pattern:?} on {haystack:?}");
        assert_eq!(
            regex.replace_all(haystack, "#").unwrap(),
            "#",
            "{pattern:?} on {haystack:?}"
        );
        // longer than the haystacks answered from sets of positions: the
        // backtracking matcher
        let long = format!("{}{haystack}", "x".repeat(200));
        assert!(regex.is_match(&long), "{pattern:?} on {long:?}");
    }
}

#[test]
fn test_case_blind_replace_all() {
    let regex = Regex::xpath(r".*\.xml", "i").unwrap();
    assert_eq!(
        regex.replace_all("file.XML", "$0.bak").unwrap(),
        "file.XML.bak"
    );
    let regex = Regex::xpath(r"(\d*)5", "i").unwrap();
    assert_eq!(regex.replace_all("555", "[$1]").unwrap(), "[55]");
    let regex = Regex::xpath(r"(\S+)@(\S+)", "i").unwrap();
    assert_eq!(regex.replace_all("a@b", "$2 at $1").unwrap(), "b at a");
    let regex = Regex::xpath(r"(.+),(.+)", "i").unwrap();
    assert_eq!(regex.replace_all("a,b", "$2,$1").unwrap(), "b,a");
    // without `i` these were and are ordinary repetitions
    let regex = Regex::xpath(r".*\.xml", "").unwrap();
    assert!(!regex.is_match("file.XML"));
    assert!(regex.is_full_match("file.xml"));
}
