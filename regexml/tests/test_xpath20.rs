// The XPath 2.0 dialect, `Regex::xpath20`: the regular expressions of XQuery
// 1.0 and XPath 2.0 Functions and Operators (Second Edition), §7.6.1, with
// the flags of §7.6.1.1. It is the XPath 3.1 dialect of `Regex::xpath`
// without what F&O 3.0 added:
//
// - non-capturing groups `(?:…)`: in the 2.0 grammar `(` always opens a
//   capturing group, and `?` is a quantifier that needs an atom before it,
//   so an unescaped `(` outside a character class is never followed by `?`;
// - the `q` flag, and anything after a `;`: "If there are characters present
//   that are not defined here as flags, then an error is raised
//   [err:FORX0001]" (§7.6.1.1, which defines `s`, `m`, `i` and `x`);
// - `.` excluding #xD: "If the s flag is not specified, the meta-character
//   . matches any character except a newline (#x0A) character" (§7.6.1.1;
//   F&O 3.1 §5.6.2 adds "or carriage return (#x0D)").
//
// The checks mirror the ones xsd-schema (`check_xpath20_regex_dialect`) and
// an XSLT 2.0 processor made on the source pattern before compiling it with
// `Regex::xpath`.

use regexml_fork::{Error, Regex};

#[track_caller]
fn assert_invalid_flags(pattern: &str, flags: &str) {
    assert!(
        matches!(Regex::xpath20(pattern, flags), Err(Error::InvalidFlags(_))),
        "expected invalid flags for {pattern:?} with {flags:?}"
    );
}

#[track_caller]
fn assert_syntax_error(pattern: &str, flags: &str) {
    assert!(
        matches!(Regex::xpath20(pattern, flags), Err(Error::Syntax(_))),
        "expected a syntax error for {pattern:?} with {flags:?}"
    );
}

#[track_caller]
fn is_match(pattern: &str, flags: &str, haystack: &str) -> bool {
    Regex::xpath20(pattern, flags)
        .unwrap_or_else(|e| panic!("{pattern:?} with {flags:?}: {e:?}"))
        .is_match(haystack)
}

#[test]
fn the_four_flags_in_any_order_and_repeated() {
    for flags in ["", "s", "m", "i", "x", "smix", "xims", "ii", "mmxs"] {
        assert!(Regex::xpath20("a", flags).is_ok(), "{flags:?}");
    }
}

#[test]
fn every_other_flag_is_invalid() {
    for flags in [
        "q", "iq", "smixq", "z", "S", "g", " ", "i ", "12345", ";", ";k", ";g", "i;k", ";K",
    ] {
        assert_invalid_flags("a", flags);
    }
    // the XPath 3.1 dialect has `q` and Saxon's `;` flags
    assert!(Regex::xpath("a", "q").is_ok());
    assert!(Regex::xpath("a", "i;k").is_ok());
}

#[test]
fn invalid_flags_are_reported_before_the_pattern() {
    assert_invalid_flags("(?:a", "q");
    assert_invalid_flags("[", ";k");
}

#[test]
fn a_group_cannot_start_with_a_question_mark() {
    for pattern in [
        "(?:a)",
        "a(?:b)c",
        "((?:a))",
        "(?i)a",
        "(?=a)",
        "(?!a)",
        "(?",
        "(?)",
        "[ab](?:c)",
        "a|(?:b)",
    ] {
        assert_syntax_error(pattern, "");
    }
    // the XPath 3.1 dialect has non-capturing groups
    assert!(Regex::xpath("(?:a)", "").is_ok());
}

#[test]
fn an_escaped_paren_or_one_in_a_class_is_an_ordinary_character() {
    // `\(?` is an escaped `(` with a `?` quantifier
    assert!(is_match(r"^\(?x$", "", "x"));
    assert!(is_match(r"^\(?x$", "", "(x"));
    // `(` and `?` are ordinary members of a character class, also of a
    // subtracted one
    assert!(is_match("[(?]", "", "?"));
    assert!(is_match("^[a-z-[(?]]$", "", "b"));
    assert!(!is_match("^[a-z-[(?]]$", "", "?"));
    // an escaped `]` does not close the class
    assert!(is_match(r"^[a\](?]+$", "", "](?"));
}

#[test]
fn under_x_the_whitespace_is_removed_first() {
    // F&O 1.0 §7.6.1.1, `x`: "whitespace characters (#x9, #xA, #xD and #x20)
    // in the regular expression are removed prior to matching with one
    // exception: whitespace characters within character class expressions
    // (charClassExpr) are not removed"
    assert_syntax_error("( ?:a)", "x");
    assert_syntax_error("(\t\n\r?:a)", "x");
    assert_syntax_error(r"\\ ( ?:a)", "x");
    assert_syntax_error(r"[a] ( ?:a)", "x");
    // without `x` the space is a character: an optional space, ':' and 'a'
    assert!(is_match("^( ?:a)$", "", ":a"));
    assert!(is_match("^( ?:a)$", "", " :a"));

    // a backslash escapes the next character that is not removed
    assert!(is_match(r"^\ (?$", "x", "("));
    assert!(is_match(r"^\ (?$", "x", ""));
    assert!(!is_match(r"^\ (?$", "x", "(("));
    assert!(is_match("^\\\t\n(?$", "x", "("));
    assert!(is_match(r"hello\ sworld", "x", "hello world"));
    // `\ a` under `x` is `\a`, which is no escape
    assert_syntax_error(r"^\ a$", "x");
    // without `x`, `\ ` is no escape either
    assert_syntax_error(r"^\ (?$", "");
    assert!(is_match(r"^\(?$", "", "("));

    // a class keeps its whitespace and its `(?`
    assert!(is_match("^[ (?]$", "x", "?"));
    assert!(is_match("^[ (?]$", "x", " "));

    // F&O's examples
    assert!(is_match("hello world", "x", "helloworld"));
    assert!(!is_match("hello[ ]world", "x", "helloworld"));
    assert!(!is_match("hello world", "x", "hello world"));

    // only those four characters are removed: a form feed (#xC) stays
    assert!(is_match("^a\u{C}b$", "x", "a\u{C}b"));
    assert!(!is_match("^a\u{C}b$", "x", "ab"));
}

#[test]
fn the_wildcard_excludes_only_the_newline() {
    // F&O 1.0 §7.6.1.1: "If the s flag is not specified, the meta-character
    // . matches any character except a newline (#x0A) character."
    assert!(is_match("^a.b$", "", "a\rb"));
    assert!(!is_match("^a.b$", "", "a\nb"));
    assert!(is_match("^a.*b$", "", "a\r\rb"));
    // in dot-all mode it matches any character whatsoever
    assert!(is_match("^a.b$", "s", "a\nb"));
    assert!(is_match("^a.b$", "s", "a\rb"));
    // a `.` in a class is a full stop
    assert!(!is_match("^[.]$", "", "\r"));
    // the other functions use the same program
    let regex = Regex::xpath20(".", "").unwrap();
    assert_eq!(regex.replace_all("a\rb\nc", "x").unwrap(), "xxx\nx");
    assert_eq!(
        regex.tokenize("\r\n\r").unwrap().collect::<Vec<_>>(),
        ["", "\n", ""]
    );
    assert!(regex.is_full_match("\r"));

    // the XPath 3.1 dialect and the XSD dialect exclude #xD too (F&O 3.1
    // §5.6.2, XSD 1.1 Part 2 §G.4.2.5: `.` is `[^\n\r]`)
    assert!(!Regex::xpath("^a.b$", "").unwrap().is_match("a\rb"));
    assert!(!Regex::xsd("a.b", "").unwrap().is_full_match("a\rb"));
}

#[test]
fn the_rest_is_the_xpath_dialect() {
    // anchors, multi-line mode, reluctant quantifiers, back-references, the
    // `\$` escape and case-insensitive mode, as in F&O 1.0 §7.6.1
    assert!(is_match("^a$", "", "a"));
    assert!(!is_match("^a$", "", "ba"));
    assert!(is_match("^b$", "m", "a\nb"));
    assert!(is_match(r"^(a+?)a$", "", "aaa"));
    assert!(is_match(r#"^('|").*\1$"#, "", "'abc'"));
    assert!(!is_match(r#"^('|").*\1$"#, "", "'abc\""));
    assert!(is_match(r"^\$$", "", "$"));
    assert!(is_match("^abc$", "i", "AbC"));
    assert!(is_match(r"^\p{Lu}+$", "", "ABC"));
    assert!(is_match(r"^[\i-[:]][\c-[:]]*$", "", "ncname"));
    // a back-reference to a group that is not closed yet is an error
    assert_syntax_error(r"(a\1)", "");
    assert_syntax_error(r"\1(a)", "");
    // an unknown block name is an error; there is no `;k` to allow it
    assert_syntax_error(r"\p{IsNoSuchBlock}", "");
    // the nesting limit of the other dialects
    let deep = format!("{}a{}", "(".repeat(65), ")".repeat(65));
    assert_syntax_error(&deep, "");
    let ok = format!("{}a{}", "(".repeat(64), ")".repeat(64));
    assert!(is_match(&ok, "", "a"));
}

#[test]
fn captures_and_the_zero_length_rule() {
    let regex = Regex::xpath20("(a)(b)?", "").unwrap();
    assert_eq!(regex.replace_all("xaby a", "[$1$2]").unwrap(), "x[ab]y [a]");
    // FORX0003 is the caller's to raise for `fn:replace` and `fn:tokenize`;
    // the engine reports it the same way in both dialects
    let regex = Regex::xpath20("a?", "").unwrap();
    assert_eq!(regex.replace_all("b", "x"), Err(Error::MatchesEmptyString));
    assert!(regex.is_match(""));
}

/// The check xsd-schema 0.2.3 made before it compiled a pattern with
/// `Regex::xpath` for `fn:matches`, `fn:replace` and `fn:tokenize`
/// (`check_xpath20_regex_dialect`, `src/xpath/functions/regex.rs`; an XSLT 2.0
/// processor has a copy): `Err(true)` for invalid flags (FORX0001),
/// `Err(false)` for a `(?` outside a class (FORX0002).
fn host_check(pattern: &str, flags: &str) -> Result<(), bool> {
    if flags.chars().any(|f| !"smix".contains(f)) {
        return Err(true);
    }
    let ignore_whitespace = flags.contains('x');
    let is_removed = |c: char| matches!(c, '\u{9}' | '\u{A}' | '\u{D}' | ' ');
    let chars: Vec<char> = pattern.chars().collect();
    let mut class_depth: usize = 0;
    let mut after_open_paren = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if ignore_whitespace && class_depth == 0 && is_removed(c) {
            continue;
        }
        let opened = after_open_paren;
        after_open_paren = false;
        match c {
            '\\' => {
                if ignore_whitespace && class_depth == 0 {
                    while chars.get(i).is_some_and(|&next| is_removed(next)) {
                        i += 1;
                    }
                }
                i += 1;
            }
            '[' => class_depth += 1,
            ']' => class_depth = class_depth.saturating_sub(1),
            '(' if class_depth == 0 => after_open_paren = true,
            '?' if opened => return Err(false),
            _ => {}
        }
    }
    Ok(())
}

/// What a host got from its check followed by `Regex::xpath`.
fn host_compile(pattern: &str, flags: &str) -> Result<Regex, Error> {
    match host_check(pattern, flags) {
        Err(true) => Err(Error::InvalidFlags(String::new())),
        Err(false) => Err(Error::Syntax(String::new())),
        Ok(()) => Regex::xpath(pattern, flags),
    }
}

/// `Regex::xpath20` accepts and rejects what the host check followed by
/// `Regex::xpath` did, with the same kind of error, and matches the same
/// strings — except that `.` now matches #xD (the only intended difference;
/// a form feed, removed by `Regex::xpath` under `x` but not by the host check
/// or by F&O, is left out of the generated patterns).
#[test]
fn agrees_with_the_host_check_followed_by_xpath() {
    // pieces rather than characters, so that `(?` and its look-alikes come up
    const PIECES: &[&str] = &[
        "a", "b", "(", ")", "?", "(?", "(?:", "( ?", "[", "]", "[(?]", "\\", "\\(", "\\ ", "-",
        "|", "*", "+", "*?", ".", " ", "\t", "\n", "\r", "^", "$", "{1}", "{0,2}", "\\1", "\\$",
    ];
    const FLAG_CHARS: &[char] = &['s', 'm', 'i', 'x', 'x', 'x', 'q', ';', 'k'];
    const INPUTS: &[&str] = &["", "a", "ab", "a(b", "?:a", "a\nb", "a\rb", " a b", "ba(?)"];
    let mut seed: u64 = 0x2026_1017;
    let mut next = |n: usize| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as usize) % n
    };
    let (mut compared, mut accepted, mut rejected_paren) = (0, 0, 0);
    for _ in 0..30_000 {
        let pattern: String = (0..1 + next(7))
            .map(|_| PIECES[next(PIECES.len())])
            .collect();
        let flags: String = (0..next(3))
            .map(|_| FLAG_CHARS[next(FLAG_CHARS.len())])
            .collect();
        if host_check(&pattern, &flags) == Err(false) {
            rejected_paren += 1;
        }
        let new = Regex::xpath20(&pattern, &flags);
        let old = host_compile(&pattern, &flags);
        compared += 1;
        match (&new, &old) {
            (Ok(new), Ok(old)) => {
                accepted += 1;
                for input in INPUTS {
                    if pattern.contains('.') && input.contains('\r') {
                        continue;
                    }
                    assert_eq!(
                        new.is_match(input),
                        old.is_match(input),
                        "{pattern:?} {flags:?} on {input:?}"
                    );
                }
            }
            (Err(Error::InvalidFlags(_)), Err(Error::InvalidFlags(_)))
            | (Err(Error::Syntax(_)), Err(Error::Syntax(_))) => {}
            _ => panic!("{pattern:?} with {flags:?}: xpath20 {new:?}, host {old:?}"),
        }
    }
    assert_eq!(compared, 30_000);
    // the corpus is not all errors, and has its share of `(?`
    assert!(accepted > 3_000, "only {accepted} patterns compiled");
    assert!(rejected_paren > 1_000, "only {rejected_paren} with `(?`");
}
