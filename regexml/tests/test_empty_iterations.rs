// Repetitions whose operand can match the empty string, and repetitions that
// have to back off into an earlier repetition, alternative or back-reference
// before what follows them can match. The expected values agree with Python's
// `re` (with F&O's rule for a back-reference to a group that has not
// matched: it matches the empty string, F&O 3.1 §5.6.1.4).
//
// F&O 3.1 §5.6.1.2: without `?` a quantifier "matches the longest possible
// substring", with it "the shortest possible substring consistent with the
// match as a whole succeeding"; either way every way of matching is tried
// before there is no match. Each call runs under a time limit, so that a
// regression fails instead of hanging.

use std::sync::mpsc;
use std::time::Duration;

use regexml_fork::Regex;

fn within_seconds<T: Send + 'static>(seconds: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(f());
    });
    receiver
        .recv_timeout(Duration::from_secs(seconds))
        .expect("the regular expression did not finish matching")
}

#[track_caller]
fn search(pattern: &'static str, haystack: &'static str, expected: bool) {
    let found = within_seconds(10, move || {
        Regex::xpath(pattern, "").unwrap().is_match(haystack)
    });
    assert_eq!(found, expected, "{pattern:?} in {haystack:?}");
}

#[track_caller]
fn full(pattern: &'static str, haystack: &'static str, expected: bool) {
    let found = within_seconds(10, move || {
        Regex::xpath(pattern, "").unwrap().is_full_match(haystack)
    });
    assert_eq!(found, expected, "{pattern:?} on {haystack:?}");
    // the captures-tracking matcher (`replace_all`) agrees
    let replaced = within_seconds(10, move || {
        Regex::xpath(&format!("^(?:{pattern})$"), "")
            .unwrap()
            .replace_all(haystack, "#")
    });
    if let Ok(replaced) = replaced {
        assert_eq!(replaced == "#", expected, "^(?:{pattern})$ on {haystack:?}");
    }
}

#[test]
fn an_inner_repetition_backs_off_under_an_outer_one() {
    // the probe: `[ab]*` has to give back the `b`
    full("([ab]*)+b", "ab", true);
    full("(.*)+B", "AB", true);
    search("([ab]*)+b", "bc", true);
    search("([ab]*)*b", "bc", true);
    search("(b?|a){1,2}b", "bca", true);
    search("([^b]?)+[^b]|[ab]ba", "bbcbb", true);
    search("(([^b]?b{0,2}){2,}){1,2}(.bc)", "bbcab", true);
    full("(a|b?)+c", "abc", true);
    full("(a|)*b", "aab", true);
    full("((a|)*b)*c", "abbc", true);
}

#[test]
fn empty_repetitions_count_towards_the_minimum() {
    full("($|a){3}", "", true);
    full("($|a){3}", "a", true);
    full("($|a){3}", "aaa", true);
    full("($|a){3}", "aaaa", false);
}

#[test]
fn a_reluctant_repetition_tries_every_alternative() {
    // a reluctant repetition used to keep only the first way each
    // repetition matched
    full("(a|ab)+?c", "abc", true);
    full("(a|ab)*?c", "abc", true);
    full("(ab|a)+?bc", "abc", true);
    full("(a?)*?b", "aab", true);
    search("(a|bc)+?d", "abcad", true);
}

#[test]
fn a_back_reference_sees_the_captures_of_its_own_path() {
    // the second alternative is reached after the first one failed at the
    // same position; zero repetitions of `(x|xy)*` must be tried again there
    full(r"(?:(a)b|a(b))(x|xy)*\2", "abb", true);
    full(r"(?:(a)b|a(b))(x|xy)*\2", "abbx", false);
    // W3C test suite fn-matches-50 (W3C bug 29253): unchanged
    full(r"((.)?a\2)+", "babadad", true);
}

#[test]
fn nested_repetitions_do_not_take_exponential_time() {
    // each took more than a minute, or two seconds, before
    full(
        r"(\w+\s?)*",
        "word word word word word word word word word word word word!",
        false,
    );
    full(
        r"(\s*\S+)*",
        "ab c ab c ab c ab c ab c ab c ab c ab c ab c ab c ",
        false,
    );
    full("(a|aa)*b", "aaaaaaaaaaaaaaaaaaaaaaaaaac", false);
    full("(a+)+b", "aaaaaaaaaaaaaaaaaaaac", false);
    full("((((((((((a)*)*)*)*)*)*)*)*)*)*", "aaaaab", false);
    full(r"((a*)*)*\2b", "aaaaaaaacb", false);
    search(r"(a|aa)*\1b", "aaaaaaaaaaaaaaaacb", true);
}
