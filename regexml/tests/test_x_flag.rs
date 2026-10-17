// The `x` flag removes whitespace from the pattern before it is read. F&O 3.1
// §5.6.2: "If present, whitespace characters (#x9, #xA, #xD and #x20) in the
// regular expression are removed prior to matching with one exception:
// whitespace characters within character class expressions (charClassExpr)
// are not removed." (F&O 1.0 §7.6.1.1 has the same sentence.) Those four
// only: a form feed (#xC) is an ordinary character, removed until 0.3.0.

use regexml_fork::Regex;

#[track_caller]
fn is_match(pattern: &str, flags: &str, haystack: &str) -> bool {
    Regex::xpath(pattern, flags)
        .unwrap_or_else(|e| panic!("{pattern:?} with {flags:?}: {e:?}"))
        .is_match(haystack)
}

#[test]
fn the_four_whitespace_characters_are_removed() {
    assert!(is_match("^a \t\n\rb$", "x", "ab"));
    assert!(!is_match("^a \t\n\rb$", "x", "a \t\n\rb"));
    // inside a class they stay
    assert!(is_match("^a[ \t]b$", "x", "a\tb"));
    // a backslash escapes the next character that is not removed
    assert!(is_match(r"hello\ sworld", "x", "hello world"));
}

#[test]
fn a_form_feed_is_an_ordinary_character() {
    for flags in ["x", "ix", "xs"] {
        assert!(is_match("^a\u{C}b$", flags, "a\u{C}b"), "{flags:?}");
        assert!(!is_match("^a\u{C}b$", flags, "ab"), "{flags:?}");
    }
    // so `(` and `?` around it are a group and a quantifier, not `(?`
    let regex = Regex::xpath("(\u{C}?:a)", "x").unwrap();
    assert_eq!(regex.replace_all("x:a", "[$1]").unwrap(), "x[:a]");
    // the same in the XSD dialect, which takes the XPath flags
    let regex = Regex::xsd("a\u{C}b", "x").unwrap();
    assert!(regex.is_full_match("a\u{C}b"));
    assert!(!regex.is_full_match("ab"));
}
