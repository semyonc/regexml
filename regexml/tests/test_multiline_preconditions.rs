// The search checks, before it tries a start position, that the operations
// every match needs can match (`ReProgram::preconditions`). After `^` it
// placed them at position 0, which is right only outside multi-line mode: in
// multi-line mode `^` also matches after a newline (F&O 3.1 §5.6.2, `m`: "^
// matches the start of any line"). The expected values agree with Python's
// `re` with `re.M`.

use regexml_fork::Regex;

#[track_caller]
fn search(pattern: &str, flags: &str, haystack: &str, expected: bool) {
    let regex = Regex::xpath(pattern, flags).unwrap();
    assert_eq!(
        regex.is_match(haystack),
        expected,
        "{pattern:?} {flags:?} in {haystack:?}"
    );
}

#[test]
fn what_follows_a_caret_in_multi_line_mode_is_not_at_the_start() {
    search(".*\n^b", "m", "x\nb", true);
    search("(.*)\n^b", "m", "x\nb", true);
    search("a*\n^b", "m", "aa\nb", true);
    search(".*\n^b", "m", "x\nc", false);
    // outside multi-line mode `^` is the start of the string
    search(".*\n^b", "", "x\nb", false);
    search("^b", "", "b", true);
}

#[test]
fn a_whole_string_match_checks_them_too() {
    let regex = Regex::xpath(".*\n^b", "m").unwrap();
    assert!(regex.is_full_match("x\nb"));
    let regex = Regex::xpath("a$\n^b", "m").unwrap();
    assert!(regex.is_full_match("a\nb"));
    assert!(!regex.is_full_match("a\nc"));
}
