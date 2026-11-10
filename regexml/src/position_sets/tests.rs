use super::*;
use crate::re_compiler::ReCompiler;
use crate::re_flags::{Language, ReFlags};
use crate::re_matcher::ReMatcher;
use crate::Regex;

fn compile(pattern: &str, flags: &str, language: Language) -> Result<ReProgram, crate::Error> {
    let flags = ReFlags::new(flags, language)?;
    ReCompiler::new(pattern.chars().collect(), flags).compile()
}

/// What the matcher answers, as `Regex` asked it before this module.
fn matcher_answers(program: &ReProgram, haystack: &str) -> (bool, bool) {
    (
        ReMatcher::new_for_answer(program, haystack).is_full_match(),
        ReMatcher::new_for_answer(program, haystack).is_match(),
    )
}

/// What this module answers, `None` for a haystack it leaves to the matcher.
fn position_answers(program: &ReProgram, haystack: &str) -> Option<(bool, bool)> {
    let full = is_full_match(program, haystack);
    let search = is_match(program, haystack);
    assert_eq!(full.is_some(), search.is_some());
    Some((full?, search?))
}

/// The answers to a whole-string match and a search, checked to be the same
/// from both, and the same through `Regex`; and that this module answers only
/// when the program is supported and the haystack short enough (and the
/// answer within `BUDGET`).
fn answers(pattern: &str, flags: &str, language: Language, haystack: &str) -> (bool, bool) {
    let program = compile(pattern, flags, language).unwrap();
    let expected = matcher_answers(&program, haystack);
    let short = haystack.chars().count() <= MAX_CHARS;
    if supports(&program) {
        let got = position_answers(&program, haystack);
        if !short {
            assert!(got.is_none(), "{pattern:?} on {haystack:?}");
        }
        if let Some(got) = got {
            assert_eq!(got, expected, "{pattern:?} {flags:?} on {haystack:?}");
        }
    }
    let regex = match language {
        Language::Xsd => Regex::xsd(pattern, flags),
        Language::XPath => Regex::xpath(pattern, flags),
        Language::XPath20 => Regex::xpath20(pattern, flags),
    }
    .unwrap();
    assert_eq!(
        (regex.is_full_match(haystack), regex.is_match(haystack)),
        expected,
        "{pattern:?} {flags:?} on {haystack:?} through `Regex`"
    );
    expected
}

fn is_supported(pattern: &str, flags: &str, language: Language) -> bool {
    supports(&compile(pattern, flags, language).unwrap())
}

/// A pattern, its dialect, and haystacks with whether it matches each whole.
type Case<'a> = (&'a str, Language, &'a [(&'a str, bool)]);

#[test]
fn targeted_patterns() {
    let a100 = "a".repeat(100);
    let key44 = "35240112345678000190550010000000011000000019";
    let key43 = &key44[1..];
    let key45 = format!("{key44}1");
    #[rustfmt::skip]
    let cases: &[Case] = &[
        ("", Language::Xsd, &[("", true), ("a", false)]),
        ("(a?)*", Language::Xsd, &[("", true), ("a", true), ("aaa", true), ("ab", false)]),
        ("(a|)*", Language::Xsd, &[("", true), ("aa", true), ("b", false)]),
        ("(a?){3}", Language::Xsd, &[("", true), ("aa", true), ("aaa", true), ("aaaa", false)]),
        ("(a?){5000}", Language::Xsd, &[("", true), ("a", true), (a100.as_str(), true), ("b", false)]),
        ("(a|){2}", Language::Xsd, &[("", true), ("aa", true), ("aaa", false)]),
        ("(ab|a){2,3}b", Language::Xsd, &[
            ("aab", true), ("abab", true), ("ababab", true), ("aaab", true),
            ("ab", false), ("abababab", false), ("aaaab", false),
        ]),
        ("a{0}", Language::Xsd, &[("", true), ("a", false)]),
        ("x{0,0}", Language::Xsd, &[("", true), ("x", false)]),
        ("x{0,0}y", Language::Xsd, &[("y", true), ("xy", false)]),
        ("[0-9]{44}", Language::Xsd, &[(key44, true), (key43, false), (key45.as_str(), false), ("", false)]),
        ("((a|b){2}){1,3}", Language::Xsd, &[
            ("ab", true), ("abba", true), ("ababab", true),
            ("", false), ("a", false), ("aba", false), ("abababab", false),
        ]),
        ("(a(b)?){2,4}c", Language::Xsd, &[("aac", true), ("ababc", true), ("aabac", true), ("ac", false), ("aaaaac", false)]),
        ("(?:ab){2,}", Language::XPath, &[("abab", true), ("ababab", true), ("ab", false), ("aba", false)]),
        ("(?:ab){2,}?c", Language::XPath, &[("ababc", true), ("abc", false)]),
        ("(a*b*)*c", Language::Xsd, &[("c", true), ("abbac", true), ("ababcb", false)]),
        ("(a+)+b", Language::Xsd, &[("ab", true), ("aaab", true), ("b", false), ("aaa", false)]),
        ("a*a", Language::Xsd, &[("a", true), ("aaa", true), ("", false)]),
        ("a*b", Language::Xsd, &[("b", true), ("aab", true), ("aa", false)]),
        ("(a|ab)(c|bcd)(d*)", Language::Xsd, &[("abcd", true), ("acd", true), ("abcdd", true), ("ab", false)]),
        ("[a-c]+?b", Language::XPath, &[("ab", true), ("abcb", true), ("abc", false)]),
        ("(a{2,3}){2}", Language::Xsd, &[("aaaa", true), ("aaaaa", true), ("aaaaaa", true), ("aaa", false), ("aaaaaaa", false)]),
        ("(?:a{2,3}?){2}?", Language::XPath, &[("aaaa", true), ("aaaaaa", true), ("aaaaaaa", false)]),
        ("0|0\\.[0-9]{2}|[1-9]{1}[0-9]{0,12}(\\.[0-9]{2})?", Language::Xsd, &[
            ("0", true), ("0.25", true), ("1234.56", true), ("12.5", false), ("01", false),
        ]),
        ("^a$", Language::Xsd, &[("^a$", true), ("a", false)]),
        ("a|", Language::Xsd, &[("", true), ("a", true), ("aa", false)]),
    ];
    for (pattern, language, haystacks) in cases {
        assert!(is_supported(pattern, "", *language), "{pattern:?}");
        for &(haystack, expected) in *haystacks {
            let (full, search) = answers(pattern, "", *language, haystack);
            assert_eq!(full, expected, "{pattern:?} on {haystack:?}");
            // a whole-string match is a match
            assert!(!full || search, "{pattern:?} on {haystack:?}");
        }
    }
}

#[test]
fn searches() {
    let cases: &[(&str, &[(&str, bool)])] = &[
        ("a", &[("", false), ("xxa", true), ("xx", false)]),
        ("", &[("", true), ("abc", true)]),
        ("ab|cd", &[("xcdx", true), ("xadx", false)]),
        ("[0-9]{3}", &[("ab12c", false), ("ab123c", true)]),
        ("a{0}", &[("b", true)]),
        ("b(a?){5000}", &[("cbd", true), ("cd", false)]),
    ];
    for (pattern, haystacks) in cases {
        for &(haystack, expected) in *haystacks {
            let (_, search) = answers(pattern, "", Language::XPath, haystack);
            assert_eq!(search, expected, "{pattern:?} on {haystack:?}");
        }
    }
}

#[test]
fn long_haystacks_go_to_the_matcher() {
    for (pattern, unit) in [
        ("a*", "a"),
        (".*", "\u{1D538}"),
        ("(ab|a)*", "a"),
        ("[0-9]+", "7"),
    ] {
        for len in [0, 1, 126, 127, 128, 129, 300] {
            let haystack = unit.repeat(len);
            let program = compile(pattern, "", Language::Xsd).unwrap();
            assert_eq!(
                position_answers(&program, &haystack).is_some(),
                len <= MAX_CHARS,
                "{pattern:?} on {len} characters"
            );
            let matches = pattern != "[0-9]+" || len > 0;
            assert_eq!(
                answers(pattern, "", Language::Xsd, &haystack),
                (matches, matches)
            );
            let mut haystack = haystack;
            haystack.push('b');
            let expected = match pattern {
                "a*" => (false, true),
                ".*" => (true, true),
                "(ab|a)*" => (len > 0, true),
                _ => (false, len > 0),
            };
            assert_eq!(answers(pattern, "", Language::Xsd, &haystack), expected);
        }
    }
    // the count is in characters, not bytes: 127 characters of four bytes
    let program = compile(".{127}", "", Language::Xsd).unwrap();
    assert_eq!(
        position_answers(&program, &"\u{1D538}".repeat(127)),
        Some((true, true))
    );
    assert_eq!(position_answers(&program, &"\u{1D538}".repeat(128)), None);
    assert_eq!(
        answers(".{127}", "", Language::Xsd, &"\u{1D538}".repeat(128)),
        (false, true)
    );
}

#[test]
fn characters_outside_the_bmp() {
    let x = "\u{1D538}";
    for (pattern, haystack, expected) in [
        (".{3}", format!("{x}a{x}"), true),
        (".{3}", format!("{x}{x}"), false),
        ("\u{1D538}{2}b", format!("{x}{x}b"), true),
        ("[\u{1D538}-\u{1D53B}]+", format!("{x}\u{1D53B}"), true),
        ("[^a]", x.to_string(), true),
        ("\\p{Lu}", x.to_string(), true),
    ] {
        assert_eq!(
            answers(pattern, "", Language::Xsd, &haystack).0,
            expected,
            "{pattern:?} on {haystack:?}"
        );
    }
}

#[test]
fn case_blind_atoms_and_classes() {
    let kelvin = "\u{212A}";
    for (pattern, haystack, expected) in [
        // ICU's simple lowercase mapping of the KELVIN SIGN is 'k'
        ("k", kelvin, true),
        (kelvin, "k", true),
        (kelvin, "K", true),
        ("abk", &format!("AB{kelvin}") as &str, true),
        ("k+", &format!("kK{kelvin}") as &str, true),
        ("[k]", kelvin, true),
        ("[a-z]{3}", &format!("A{kelvin}z") as &str, true),
        ("(?:ab){2}", "AbaB", true),
        ("ab", "ac", false),
        ("\u{1D538}", "\u{1D552}", false),
    ] {
        assert!(is_supported(pattern, "i", Language::XPath));
        assert_eq!(
            answers(pattern, "i", Language::XPath, haystack).0,
            expected,
            "{pattern:?} on {haystack:?}"
        );
    }
    // without the flag, no case variants
    assert_eq!(answers("k", "", Language::XPath, kelvin), (false, false));
}

#[test]
fn unsupported_programs() {
    for (pattern, flags) in [("^a", ""), ("a$", ""), ("(a)\\1", ""), ("(?:a|^b)c", "")] {
        assert!(
            !is_supported(pattern, flags, Language::XPath),
            "{pattern:?}"
        );
    }
    for (pattern, flags) in [
        ("a", "q"),
        ("a.b", "q"),
        ("^a$", "q"),
        ("^$", ""),
        ("", "q"),
    ] {
        let supported = is_supported(pattern, flags, Language::XPath);
        assert_eq!(supported, flags == "q", "{pattern:?}");
    }
    // in XSD, `^` and `$` are ordinary characters
    assert!(is_supported("^a$", "", Language::Xsd));
}

// An empty pattern with the `q` flag compiles to an empty atom, which matches
// the empty string everywhere. The matcher's preconditions
// (`ReMatcher::check_preconditions`) used to look for it only at the positions
// before the last character, of which the empty haystack has none, so its
// whole-string match was rejected (0.3.0); they now look at the end as well.
#[test]
fn empty_literal_matches_the_empty_haystack() {
    let program = compile("", "q", Language::XPath).unwrap();
    assert!(supports(&program));
    assert_eq!(matcher_answers(&program, ""), (true, true));
    assert_eq!(answers("", "q", Language::XPath, ""), (true, true));
    assert_eq!(answers("", "q", Language::XPath, "a"), (false, true));
    assert_eq!(answers("", "iq", Language::XPath, ""), (true, true));
    // the empty atom is the identity on sets of positions
    let haystack = Haystack::new(&[], &program, true);
    assert_eq!(haystack.ends(&program.operation, 1), 1);
}

// Under `i`, `ReCompiler::no_ambiguity` used to make a repetition of an atom
// possessive from the ICU case closures of the first characters, which leave
// out the character itself ('a' gives {'A'}, 'A' gives {'a'}) and are not
// how atoms compare (equal simple lowercase mappings: 'i' matches 'İ',
// U+0130). So `a*A` took the 'A' the atom after it needed and rejected "aA"
// (0.3.0). A case-blind atom's initial class is now every character, so
// these are ordinary repetitions and match; without `i`, `a*b` stays
// possessive.
#[test]
fn case_blind_atoms_are_not_made_possessive() {
    for (pattern, haystack) in [
        ("a*A", "aA"),
        ("a?A", "A"),
        ("i*\u{130}", "\u{130}"),
        ("a+A", "AaA"),
    ] {
        let program = compile(pattern, "i", Language::XPath).unwrap();
        assert!(supports(&program));
        assert!(
            !matches!(&program.operation, Operation::Sequence(sequence)
            if matches!(sequence.operations[0], Operation::UnambiguousRepeat(_))),
            "{pattern:?}"
        );
        assert_eq!(
            answers(pattern, "i", Language::XPath, haystack),
            (true, true),
            "{pattern:?}"
        );
        for other in ["aaA", "\u{130}", "i\u{130}", "I", "ii", "", "b"] {
            answers(pattern, "i", Language::XPath, other);
        }
        // the old engine on a haystack longer than this module takes
        let long = format!(
            "{}{haystack}",
            haystack
                .chars()
                .next()
                .unwrap()
                .to_string()
                .repeat(MAX_CHARS)
        );
        assert_eq!(
            answers(pattern, "i", Language::XPath, &long).0,
            pattern != "a?A",
            "{pattern:?}"
        );
    }
    let program = compile("a*b", "", Language::XPath).unwrap();
    assert!(matches!(&program.operation, Operation::Sequence(sequence)
        if matches!(sequence.operations[0], Operation::UnambiguousRepeat(_))));
    assert_eq!(answers("a*A", "", Language::XPath, "aA"), (true, true));
    assert_eq!(answers("a*A", "", Language::XPath, "AA"), (false, true));
}

#[test]
fn possessive_repetitions() {
    // possessive where the next operation cannot start with the operand
    for (pattern, haystacks) in [
        ("a*b", &["b", "ab", "aab", "aaba", "ba"][..]),
        ("a{2,4}b", &["ab", "aab", "aaaab", "aaaaab"]),
        ("[0-9]*x", &["x", "12x", "1x2", "12"]),
        ("(?:ab)*c", &["c", "ababc", "abac"]),
        ("a{3}", &["aa", "aaa", "aaaa"]),
        ("b(?:a{2})*", &["b", "baa", "baaa", "baaaa"]),
    ] {
        let program = compile(pattern, "", Language::XPath).unwrap();
        assert!(supports(&program));
        for haystack in haystacks {
            answers(pattern, "", Language::XPath, haystack);
        }
    }
}

// Nested repetitions: the steps multiply. `(((a*)*)*)*` doubled per level
// before the minimum's fixed point ended a repetition at once (the compiler
// gives a repetition of an operand that matches the empty string a minimum of
// one), and `((a){1,2}){1,2}…` doubles per level still, so past `BUDGET`
// evaluations the matcher answers. Either way the answers are the matcher's.
#[test]
fn nested_repetitions_stay_within_the_budget() {
    for depth in [1, 4, 8, 16, 32, 60] {
        let pattern = format!("{}a*{}", "(".repeat(depth), ")*".repeat(depth));
        for haystack in ["", "a", "aaab", "aaaa", &"a".repeat(MAX_CHARS)] {
            answers(&pattern, "", Language::Xsd, haystack);
            let program = compile(&pattern, "", Language::Xsd).unwrap();
            assert!(
                position_answers(&program, haystack).is_some(),
                "{depth} {haystack:?}"
            );
        }
    }
    // `a` repeated once or twice, d levels deep: each level evaluates its
    // operand on two sets that differ, 2^d evaluations
    let mut left_to_the_matcher = 0;
    for depth in [4, 8, 10, 11, 12, 14] {
        let pattern = format!("{}a{}", "(".repeat(depth), "){1,2}".repeat(depth));
        let program = compile(&pattern, "", Language::Xsd).unwrap();
        for haystack in ["", "a", "aaaaa", &"a".repeat(16), "b"] {
            answers(&pattern, "", Language::Xsd, haystack);
            if position_answers(&program, haystack).is_none() {
                left_to_the_matcher += 1;
            }
        }
    }
    assert!(left_to_the_matcher > 0, "the budget never ran out");
}

/// Real expressions take a small part of `BUDGET`: what the sets of positions
/// are for stays within it with room to spare.
#[test]
fn real_expressions_take_a_small_part_of_the_budget() {
    let cases: &[(&str, &str)] = &[
        ("[0-9]{1,15}", "123456789"),
        (r"0|0\.[0-9]{2}|[1-9]{1}[0-9]{0,12}(\.[0-9]{2})?", "1234.56"),
        ("[!-\u{FF}]{1}[ -\u{FF}]{0,}[!-\u{FF}]{1}|[!-\u{FF}]{1}", "PRODUTO TESTE NUMERO 12 COM UM NOME BEM MAIS LONGO DO QUE O NORMAL PARA TESTAR 0123456789 ABCDEFGHIJ"),
        ("[0-9]{44}", "35240112345678000190550010000000011000000019"),
        (r"(((20(([02468][048])|([13579][26]))-02-29))|(20[0-9][0-9])-((((0[1-9])|(1[0-2]))-((0[1-9])|(1\d)|(2[0-8])))|((((0[13578])|(1[02]))-31)|(((0[1,3-9])|(1[0-2]))-(29|30)))))T(20|21|22|23|[0-1]\d):[0-5]\d:[0-5]\d([\-,\+](0[0-9]|10|11):00|([\+](12):00))", "2024-01-15T10:30:00-03:00"),
        (r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}", "someone.with.a.long.name@sub.domain.example.com"),
        (r"(https?|ftp)://[^\s/$.?#].[^\s]*", "https://www.example.com/path/to/some/resource?query=value&other=1"),
        (r"(\p{L}+\s?)+", "Uma frase com varias palavras para testar repeticao aninhada de grupos"),
        (r"((a|b|c)*d)*(e|f)?", "abcdabcdccccddabcdaaaadf"),
        (r"([A-Z]{2}[0-9]{2}){1,10}(-[A-Z0-9]{1,8}){0,5}", "AB12CD34EF56-X1-YY22-ZZZ333"),
        (r"(\d{1,3}\.){3}\d{1,3}", "192.168.100.254"),
        (r"[0-9]{2}([0-9]{2})?(-[0-9]{2}){0,2}", "2024-01-15"),
    ];
    for (pattern, haystack) in cases {
        let program = compile(pattern, "", Language::Xsd).unwrap();
        assert!(supports(&program), "{pattern}");
        let mut buffer = ['\0'; MAX_CHARS];
        let chars = collect(haystack, &mut buffer).unwrap();
        let h = Haystack::new(chars, &program, true);
        h.ends(&program.operation, 1);
        let full = BUDGET - h.budget.get();
        let h = Haystack::new(chars, &program, false);
        h.ends(&program.operation, below(chars.len() + 1));
        let search = BUDGET - h.budget.get();
        assert!(
            full <= BUDGET / 8 && search <= BUDGET / 8,
            "{full} {search} {pattern}"
        );
    }
}

/// How many positions answering `pattern` on `haystack` looks up for atoms
/// and character classes, whole-string match and search together.
fn lookups(pattern: &str, haystack: &str) -> usize {
    let program = compile(pattern, "", Language::XPath).unwrap();
    assert!(supports(&program), "{pattern}");
    LOOKUPS.with(|lookups| lookups.set(0));
    assert_eq!(is_full_match(&program, haystack), Some(true), "{pattern}");
    assert_eq!(is_match(&program, haystack), Some(true), "{pattern}");
    LOOKUPS.with(Cell::get)
}

// A position is looked up for a repeated atom or class once, when a
// repetition first starts from it. Before, every evaluation of a repetition
// looked up its whole window (to the end of the haystack, for no upper
// bound), so `\w+` inside an outer repetition scanned the rest of the
// haystack at each outer step: quadratic in the length of the haystack,
// and for `(\w+\s?)+` on 126 characters five times the time of 0.3.0.
#[test]
fn repetitions_look_up_each_position_once() {
    // one-letter words, "a b c ...", of n characters
    let words = |n: usize| -> String {
        let mut s: String = ('a'..='z')
            .cycle()
            .take(n.div_ceil(2))
            .flat_map(|c| [c, ' '])
            .collect();
        s.truncate(n);
        s
    };
    let numbers = |n: usize| -> String {
        let mut s = "12.5 -3 +4.25 7 0.001 -99 ".repeat(n.div_ceil(26));
        s.truncate(n);
        s.trim_end_matches(|c: char| !c.is_ascii_digit())
            .to_string()
    };
    let cases: [(&str, &dyn Fn(usize) -> String); 4] = [
        (r"(\w+\s?)+", &words),
        (r"(\p{L}+\s?)+", &words),
        (r"(\s*\w+\s*)*", &words),
        (r"((\+|-)?\d+(\.\d+)?\s*)+", &numbers),
    ];
    for (pattern, haystack) in cases {
        let costs: Vec<(usize, usize)> = [31, 63, 126]
            .into_iter()
            .map(|n| {
                let haystack = haystack(n);
                (haystack.len(), lookups(pattern, &haystack))
            })
            .collect();
        for &(n, cost) in &costs {
            // a few lookups per position and per class or atom; looking up
            // the whole window took 4,657 for `(\w+\s?)+` on 126 characters
            assert!(
                cost <= 20 * n,
                "{pattern} on {n} characters: {cost} lookups"
            );
        }
        // four times the haystack, about four times the lookups
        assert!(costs[2].1 <= 5 * costs[0].1, "{pattern}: {costs:?}");
    }
    // a fixed count looks up each position it needs once, in each answer
    let digits = "35240112345678000190550010000000011000000019";
    assert_eq!(lookups("[0-9]{44}", digits), 2 * 44);
}

// Repetition counts near `usize::MAX`: the lengths the compiler and the
// matcher compute from them saturate instead of wrapping (or panicking in a
// debug build), so the matcher's answers are the sets of positions' and do
// not depend on the length of the haystack.
#[test]
fn huge_repetition_counts() {
    let max = usize::MAX;
    let half = usize::MAX / 2;
    let quarter = usize::MAX / 4 + 1;
    let long = |n: usize| format!("zz{}", "ab".repeat(n));
    let cases: &[(String, String, (bool, bool))] = &[
        (format!("..(ab){{1,{half}}}"), "zzab".into(), (true, true)),
        (format!("..(ab){{1,{half}}}"), long(60), (true, true)),
        (format!("..(ab){{1,{half}}}"), long(70), (true, true)),
        (format!("c(ab){{1,{half}}}"), "zzcab".into(), (false, true)),
        (
            format!("(abcd){{0,{quarter}}}"),
            "abcdabcdabcd".into(),
            (true, true),
        ),
        (format!("abc(x){{{}}}", max - 2), "".into(), (false, false)),
        (format!("abc(x){{{}}}", max - 2), "a".into(), (false, false)),
        (
            format!("abc(x){{{}}}", max - 2),
            "abcx".into(),
            (false, false),
        ),
    ];
    for (pattern, haystack, expected) in cases {
        assert_eq!(
            answers(pattern, "", Language::Xsd, haystack),
            *expected,
            "{pattern:?} on {haystack:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Differential test: random patterns and haystacks, this module against the
// matcher, and both against a reference that evaluates the generated
// expression itself (not the compiled program), from the definitions of XSD
// 1.1 Part 2 Appendix G.

/// SplitMix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

/// A generated expression.
#[derive(Debug)]
enum Node {
    Char(char),
    // an index into `CLASSES`
    Class(usize),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    // capturing or not (XPath only)
    Group(Box<Node>, bool),
    Quant(Box<Node>, Quantifier),
    // XPath only; not supported by this module
    Anchor(char),
    BackRef(usize),
}

#[derive(Debug, Clone, Copy)]
struct Quantifier {
    min: usize,
    max: usize,
    text: Option<&'static str>,
    reluctant: bool,
}

#[rustfmt::skip]
const CHARS: &[char] = &[
    'a', 'a', 'a', 'b', 'b', 'c', 'A', 'k', 'K', '\u{212A}', 'i', '\u{130}', '\u{1D538}', '1',
];

#[rustfmt::skip]
const HAYSTACK_CHARS: &[char] = &[
    'a', 'a', 'a', 'a', 'b', 'b', 'b', 'c', 'A', 'B', 'k', 'K', '\u{212A}', 'i', 'I', '\u{130}',
    '1', '7', '\u{663}', ' ', '\n', '\u{1D538}',
];

/// The classes, with characters in them to make haystacks from.
const CLASSES: &[(&str, &[char])] = &[
    (".", &['a', 'b', 'K', '1', '\u{1D538}']),
    ("[a-c]", &['a', 'b', 'c']),
    ("[^b]", &['a', 'c', 'A', '\u{130}', '\n']),
    ("\\d", &['1', '7', '\u{663}']),
    ("[ab]", &['a', 'b']),
    ("[A-Z]", &['A', 'B', 'K']),
    ("\\w", &['a', '1', 'K']),
    ("[k]", &['k']),
    ("\\p{Lu}", &['A', 'K', '\u{130}']),
    ("[a-c-[b]]", &['a', 'c']),
    ("\\s", &[' ', '\n']),
    ("[\u{1D538}a]", &['\u{1D538}', 'a']),
    ("[^\\d]", &['a', 'b']),
];

struct Generator {
    rng: Rng,
    xpath: bool,
    // capturing groups opened so far, for back-references
    groups: usize,
}

impl Generator {
    fn node(&mut self, depth: usize) -> Node {
        if depth == 0 || self.rng.chance(30) {
            return self.leaf();
        }
        match self.rng.below(10) {
            0..=2 => Node::Seq(
                (0..2 + self.rng.below(3))
                    .map(|_| self.node(depth - 1))
                    .collect(),
            ),
            3 | 4 => Node::Alt(
                (0..2 + self.rng.below(2))
                    .map(|_| {
                        if self.rng.chance(20) {
                            Node::Seq(Vec::new())
                        } else {
                            self.node(depth - 1)
                        }
                    })
                    .collect(),
            ),
            5 => {
                let capturing = !self.xpath || self.rng.chance(50);
                if capturing {
                    self.groups += 1;
                }
                Node::Group(Box::new(self.node(depth - 1)), capturing)
            }
            _ => {
                let quantifier = self.quantifier();
                Node::Quant(Box::new(self.node(depth - 1)), quantifier)
            }
        }
    }

    fn leaf(&mut self) -> Node {
        if self.xpath && self.rng.chance(3) {
            return if self.groups > 0 && self.rng.chance(50) {
                Node::BackRef(1 + self.rng.below(self.groups))
            } else {
                Node::Anchor(self.rng.pick(&['^', '$']))
            };
        }
        if self.rng.chance(55) {
            Node::Char(self.rng.pick(CHARS))
        } else {
            Node::Class(self.rng.below(CLASSES.len()))
        }
    }

    fn quantifier(&mut self) -> Quantifier {
        const MAX: usize = usize::MAX;
        let (min, max, text) = match self.rng.below(10) {
            0 => (0, 1, Some("?")),
            1 | 2 => (0, MAX, Some("*")),
            3 => (1, MAX, Some("+")),
            4 => {
                let m = self.rng.below(4);
                (m, m, None)
            }
            5 => (self.rng.below(4), MAX, None),
            6 => (0, 0, None),
            _ => {
                let m = self.rng.below(3);
                (m, m + self.rng.below(4), None)
            }
        };
        Quantifier {
            min,
            max,
            text,
            reluctant: self.xpath && self.rng.chance(25),
        }
    }
}

fn render(node: &Node, xpath: bool, out: &mut String) {
    match node {
        Node::Char(c) => out.push(*c),
        Node::Class(i) => out.push_str(CLASSES[*i].0),
        Node::Seq(nodes) => {
            for node in nodes {
                // a choice in a sequence needs a group
                if matches!(node, Node::Alt(_)) {
                    out.push_str(if xpath { "(?:" } else { "(" });
                    render(node, xpath, out);
                    out.push(')');
                } else {
                    render(node, xpath, out);
                }
            }
        }
        Node::Alt(nodes) => {
            for (i, node) in nodes.iter().enumerate() {
                if i > 0 {
                    out.push('|');
                }
                render(node, xpath, out);
            }
        }
        Node::Group(node, capturing) => {
            out.push_str(if *capturing { "(" } else { "(?:" });
            render(node, xpath, out);
            out.push(')');
        }
        Node::Quant(node, quantifier) => {
            // a quantifier applies to an atom
            if matches!(**node, Node::Char(_) | Node::Class(_) | Node::Group(..)) {
                render(node, xpath, out);
            } else {
                out.push_str(if xpath { "(?:" } else { "(" });
                render(node, xpath, out);
                out.push(')');
            }
            match quantifier.text {
                Some(text) => out.push_str(text),
                None if quantifier.max == usize::MAX => {
                    out.push_str(&format!("{{{},}}", quantifier.min))
                }
                None if quantifier.min == quantifier.max && quantifier.min % 2 == 0 => {
                    out.push_str(&format!("{{{}}}", quantifier.min))
                }
                None => out.push_str(&format!("{{{},{}}}", quantifier.min, quantifier.max)),
            }
            if quantifier.reluctant {
                out.push('?');
            }
        }
        Node::Anchor(c) => out.push(*c),
        // in a group, so that a digit after it is not part of its number
        Node::BackRef(group) => out.push_str(&format!("(?:\\{group})")),
    }
}

/// A case variant the `i` flag makes match, sometimes.
fn case_variant(rng: &mut Rng, c: char) -> char {
    let variants: &[char] = match c {
        'a' => &['A'],
        'A' => &['a'],
        'b' => &['B'],
        'k' => &['K', '\u{212A}'],
        'K' => &['k', '\u{212A}'],
        '\u{212A}' => &['k', 'K'],
        'i' => &['I', '\u{130}'],
        _ => &[],
    };
    if variants.is_empty() || rng.chance(50) {
        c
    } else {
        rng.pick(variants)
    }
}

/// A string the expression matches, or nearly: back-references and anchors
/// are left out. Unbounded repetitions go to `extra` more than the minimum.
fn sample(node: &Node, rng: &mut Rng, case_blind: bool, extra: usize, out: &mut Vec<char>) {
    if out.len() > 200 {
        return;
    }
    let mut push = |rng: &mut Rng, c: char| {
        out.push(if case_blind { case_variant(rng, c) } else { c });
    };
    match node {
        Node::Char(c) => push(rng, *c),
        Node::Class(i) => {
            let c = rng.pick(CLASSES[*i].1);
            push(rng, c)
        }
        Node::Seq(nodes) => nodes
            .iter()
            .for_each(|node| sample(node, rng, case_blind, extra, out)),
        Node::Alt(nodes) => {
            let node = &nodes[rng.below(nodes.len())];
            sample(node, rng, case_blind, extra, out)
        }
        Node::Group(node, _) => sample(node, rng, case_blind, extra, out),
        Node::Quant(node, quantifier) => {
            let spread = quantifier.max.saturating_sub(quantifier.min).min(extra);
            let count = quantifier.min + rng.below(spread + 1);
            for _ in 0..count {
                sample(node, rng, case_blind, extra, out);
            }
        }
        Node::Anchor(_) | Node::BackRef(_) => {}
    }
}

/// The haystacks to try an expression on.
fn haystacks(node: &Node, rng: &mut Rng, case_blind: bool, literal: Option<&str>) -> Vec<String> {
    let mut haystacks = vec![String::new()];
    if let Some(literal) = literal {
        haystacks.push(literal.to_string());
    }
    for _ in 0..6 {
        let mut chars = Vec::new();
        sample(node, rng, case_blind, 3, &mut chars);
        chars.truncate(30);
        if rng.chance(50) {
            // one edit: insert, delete or replace a character
            let at = rng.below(chars.len() + 1);
            match rng.below(3) {
                0 => chars.insert(at, rng.pick(HAYSTACK_CHARS)),
                1 if at < chars.len() => {
                    chars.remove(at);
                }
                _ if at < chars.len() => chars[at] = rng.pick(HAYSTACK_CHARS),
                _ => {}
            }
        }
        haystacks.push(chars.into_iter().collect());
    }
    for _ in 0..5 {
        let len = rng.below(21);
        haystacks.push((0..len).map(|_| rng.pick(HAYSTACK_CHARS)).collect());
    }
    // around the longest haystack this module answers
    if rng.chance(25) {
        let len = 124 + rng.below(7);
        haystacks.push((0..len).map(|_| rng.pick(&['a', 'b'])).collect());
        let mut chars = Vec::new();
        sample(node, rng, case_blind, 60, &mut chars);
        chars.truncate(124 + rng.below(7));
        haystacks.push(chars.into_iter().collect());
    }
    haystacks
}

/// Whether the expression has an anchor or a back-reference, which the
/// reference does not evaluate.
fn has_anchor_or_back_reference(node: &Node) -> bool {
    match node {
        Node::Anchor(_) | Node::BackRef(_) => true,
        Node::Char(_) | Node::Class(_) => false,
        Node::Seq(nodes) | Node::Alt(nodes) => nodes.iter().any(has_anchor_or_back_reference),
        Node::Group(node, _) | Node::Quant(node, _) => has_anchor_or_back_reference(node),
    }
}

/// The ends of the generated expression itself, by the definitions of Part
/// 2 Appendix G: a character matches itself (under `i`, as the matcher
/// compares, `equal_case_blind`), a class its characters (the class the
/// compiler builds for it alone, with the same flags), and S{n,m} is the
/// union of the ends after k repetitions, n <= k <= m — computed one k after
/// another, until there are none or they are those of k - 1 (then they stay).
struct Reference<'a> {
    chars: &'a [char],
    case_blind: bool,
    classes: &'a [CharacterClass],
}

impl Reference<'_> {
    fn ends(&self, node: &Node, from: Positions) -> Positions {
        let n = self.chars.len();
        let one = |matches: &dyn Fn(char) -> bool| {
            let mut ends = 0;
            for p in 0..n {
                if from & (1 << p) != 0 && matches(self.chars[p]) {
                    ends |= 1 << (p + 1);
                }
            }
            ends
        };
        match node {
            Node::Char(c) => one(&|h| {
                if self.case_blind {
                    equal_case_blind(h, *c)
                } else {
                    h == *c
                }
            }),
            Node::Class(i) => one(&|h| self.classes[*i].contains(h)),
            Node::Seq(nodes) => nodes.iter().fold(from, |at, node| self.ends(node, at)),
            Node::Alt(nodes) => nodes
                .iter()
                .fold(0, |ends, node| ends | self.ends(node, from)),
            Node::Group(node, _) => self.ends(node, from),
            Node::Quant(node, quantifier) => {
                let mut at = from;
                let mut ends = if quantifier.min == 0 { from } else { 0 };
                for k in 1..=quantifier.max {
                    let next = self.ends(node, at);
                    let same = next == at;
                    at = next;
                    if k >= quantifier.min || same {
                        ends |= at;
                    }
                    if at == 0 || same {
                        break;
                    }
                }
                ends
            }
            Node::Anchor(_) | Node::BackRef(_) => unreachable!(),
        }
    }
}

/// The classes of `CLASSES` as the compiler builds them with `flags`.
fn compiled_classes(flags: &str, language: Language) -> Vec<CharacterClass> {
    CLASSES
        .iter()
        .map(|(class, _)| {
            let program = compile(class, flags, language).unwrap();
            let Operation::Sequence(sequence) = &program.operation else {
                panic!("{class:?}")
            };
            let Operation::CharClass(class) = &sequence.operations[0] else {
                panic!("{class:?}")
            };
            class.character_class.clone()
        })
        .collect()
}

#[derive(Debug, Default)]
struct Tally {
    patterns: usize,
    compile_errors: usize,
    supported: usize,
    unsupported: usize,
    pairs: usize,
    answered_here: usize,
    left_to_matcher: usize,
    full_matches: usize,
    reference_pairs: usize,
    // where the matcher (and so this module) differs from the reference
    matcher_differs: Vec<String>,
    // where the possessive repetitions explain it: as ordinary repetitions,
    // this module agrees with the reference
    explained_by_possessive: usize,
    // differences without the `i` flag
    case_sensitive_differences: usize,
}

fn differential(seed: u64, patterns: usize) -> Tally {
    let mut rng = Rng(seed);
    let mut tally = Tally::default();
    let mut classes = std::collections::HashMap::new();
    for _ in 0..patterns {
        let xpath = rng.chance(50);
        let language = if xpath {
            Language::XPath
        } else {
            Language::Xsd
        };
        let flags = if xpath && rng.chance(5) {
            rng.pick(&["q", "iq"])
        } else {
            rng.pick(&["", "", "", "i", "i", "s", "is"])
        };
        let mut generator = Generator {
            rng: Rng(rng.next()),
            xpath,
            groups: 0,
        };
        let node = generator.node(4);
        let mut pattern = String::new();
        render(&node, xpath, &mut pattern);
        tally.patterns += 1;
        let Ok(program) = compile(&pattern, flags, language) else {
            tally.compile_errors += 1;
            continue;
        };
        let supported = supports(&program);
        if supported {
            tally.supported += 1;
        } else {
            tally.unsupported += 1;
        }
        let literal = flags.contains('q');
        let case_blind = flags.contains('i');
        let reference = !literal && !has_anchor_or_back_reference(&node);
        // (an anchor or a back-reference may be compiled away, as in `^?`)
        if literal {
            assert_eq!(supported, !pattern.is_empty(), "{pattern:?} {flags:?}");
        } else if !has_anchor_or_back_reference(&node) {
            assert!(supported, "{pattern:?} {flags:?}");
        }
        let classes = classes
            .entry((flags.replace('q', ""), xpath))
            .or_insert_with(|| compiled_classes(&flags.replace('q', ""), language));
        // through `Regex` too, for some
        let regex = rng.chance(10).then(|| {
            if xpath {
                Regex::xpath(&pattern, flags).unwrap()
            } else {
                Regex::xsd(&pattern, flags).unwrap()
            }
        });
        for haystack in haystacks(&node, &mut rng, case_blind, literal.then_some(&*pattern)) {
            tally.pairs += 1;
            let expected = matcher_answers(&program, &haystack);
            tally.full_matches += expected.0 as usize;
            if let Some(regex) = &regex {
                assert_eq!(
                    (regex.is_full_match(&haystack), regex.is_match(&haystack)),
                    expected,
                    "{pattern:?} {flags:?} on {haystack:?} through `Regex`"
                );
            }
            if !supported {
                tally.left_to_matcher += 1;
                continue;
            }
            let chars: Vec<char> = haystack.chars().collect();
            let got = position_answers(&program, &haystack);
            assert_eq!(got.is_some(), chars.len() <= MAX_CHARS);
            let Some(got) = got else {
                tally.left_to_matcher += 1;
                continue;
            };
            tally.answered_here += 1;
            assert_eq!(
                got, expected,
                "{pattern:?} {flags:?} {language:?} on {haystack:?}: sets of positions \
                 (whole, search) against the matcher"
            );
            if !reference {
                continue;
            }
            tally.reference_pairs += 1;
            let reference = Reference {
                chars: &chars,
                case_blind,
                classes,
            };
            let ends = reference.ends(&node, 1);
            let full = (ends >> chars.len()) & 1 == 1;
            let search = reference.ends(&node, below(chars.len() + 1)) != 0;
            if (full, search) != got {
                POSSESSIVE_AS_COUNTED.with(|counted| counted.set(true));
                let counted = position_answers(&program, &haystack).unwrap();
                POSSESSIVE_AS_COUNTED.with(|counted| counted.set(false));
                if counted == (full, search) {
                    tally.explained_by_possessive += 1;
                }
                if !case_blind {
                    tally.case_sensitive_differences += 1;
                }
                tally.matcher_differs.push(format!(
                    "{pattern:?} {flags:?} {language:?} on {haystack:?}: matcher {got:?}, \
                     reference {:?}, possessive as counted {counted:?}",
                    (full, search)
                ));
            }
        }
    }
    tally
}

fn report(tally: &Tally) {
    eprintln!(
        "{} patterns: {} compile errors, {} supported, {} left to the matcher; {} pairs \
         ({} whole-string matches): {} answered here, {} left to the matcher; {} against the \
         reference, of which the matcher differs from it on {} ({} explained by possessive \
         repetitions, {} without the `i` flag)",
        tally.patterns,
        tally.compile_errors,
        tally.supported,
        tally.unsupported,
        tally.pairs,
        tally.full_matches,
        tally.answered_here,
        tally.left_to_matcher,
        tally.reference_pairs,
        tally.matcher_differs.len(),
        tally.explained_by_possessive,
        tally.case_sensitive_differences,
    );
    for line in tally.matcher_differs.iter().take(20) {
        eprintln!("  {line}");
    }
}

#[test]
fn differential_against_the_matcher() {
    let tally = differential(0x5EED_2026_1109, 9_000);
    report(&tally);
    assert!(tally.pairs >= 100_000, "{}", tally.pairs);
    assert!(tally.unsupported > 0 && tally.left_to_matcher > tally.unsupported);
    // The matcher, and so this module, agree with the reference everywhere
    // (in 0.3.0 they did not under `i`: see
    // `case_blind_atoms_are_not_made_possessive`).
    assert!(tally.reference_pairs >= 50_000, "{}", tally.reference_pairs);
    assert!(
        tally.matcher_differs.is_empty(),
        "differences from the reference: {:#?}",
        &tally.matcher_differs[..tally.matcher_differs.len().min(20)]
    );
}

// A larger run, for an optimized build:
// `cargo test --release -p regexml-fork differential_large -- --ignored --nocapture`
#[test]
#[ignore]
fn differential_large() {
    let tally = differential(0xD1FF_E2E2_0001, 200_000);
    report(&tally);
    assert!(
        tally.matcher_differs.is_empty(),
        "differences from the reference: {:#?}",
        &tally.matcher_differs[..tally.matcher_differs.len().min(20)]
    );
}
