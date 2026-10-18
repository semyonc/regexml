// `Error::kind`: what kind of error a compile or an operation reported, for a
// caller to map to its own error codes without reading the message. The
// F&O 3.1 codes are FORX0001 (flags), FORX0002 (pattern), FORX0003 (matches
// the zero-length string) and FORX0004 (replacement string); the nesting
// limit and unknown block names are syntax errors with a kind of their own,
// because a caller may treat them differently (an implementation limit; the
// `;k` flag of XSD 1.1 Part 2 §G.4.2.4).

use std::collections::HashSet;

use regexml_fork::{Error, ErrorKind, Regex};

#[track_caller]
fn kind(result: Result<Regex, Error>) -> ErrorKind {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.kind(),
    }
}

fn groups(depth: usize) -> String {
    format!("{}a{}", "(".repeat(depth), ")".repeat(depth))
}

fn subtractions(depth: usize) -> String {
    // `[a-z-[a-z-[…[a]…]]]`: `depth` classes, one inside the other
    format!("{}[a]{}", "[a-z-".repeat(depth - 1), "]".repeat(depth - 1))
}

#[test]
fn invalid_flags() {
    assert_eq!(kind(Regex::xpath("a", "z")), ErrorKind::InvalidFlags);
    assert_eq!(kind(Regex::xpath("a", ";z")), ErrorKind::InvalidFlags);
    assert_eq!(kind(Regex::xsd("a", "q")), ErrorKind::InvalidFlags);
    assert_eq!(kind(Regex::xpath20("a", "q")), ErrorKind::InvalidFlags);
    assert_eq!(kind(Regex::xpath20("a", ";k")), ErrorKind::InvalidFlags);
}

#[test]
fn syntax() {
    for pattern in [
        "(a", "[a", "a{2,1}", ")", "*", r"\1(a)", r"(a\1)", r"[\1]", r"\p{Xx}", r"\p{Foo}",
    ] {
        assert_eq!(
            kind(Regex::xpath(pattern, "")),
            ErrorKind::Syntax,
            "{pattern:?}"
        );
    }
    assert_eq!(kind(Regex::xsd("a*?", "")), ErrorKind::Syntax);
    assert_eq!(kind(Regex::xsd(r"\$", "")), ErrorKind::Syntax);
    assert_eq!(kind(Regex::xsd("(?:a)", "")), ErrorKind::Syntax);
    assert_eq!(kind(Regex::xpath20("(?:a)", "")), ErrorKind::Syntax);
    // `Is` followed by something that is not a block name (XSD 1.1 IsBlock is
    // 'Is' [a-zA-Z0-9#x2D]+) is a syntax error, `;k` or not
    for flags in ["", ";k"] {
        assert_eq!(kind(Regex::xsd(r"\p{Is_X}", flags)), ErrorKind::Syntax);
        assert_eq!(kind(Regex::xsd(r"\p{Is}", flags)), ErrorKind::Syntax);
    }
}

#[test]
fn nesting_too_deep() {
    for pattern in [
        groups(65),
        subtractions(65),
        format!("({})", subtractions(64)),
    ] {
        let error = Regex::xpath(&pattern, "").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::NestingTooDeep, "{pattern}");
        // still the variant it was
        assert!(matches!(error, Error::Syntax(_)));
        assert_eq!(kind(Regex::xsd(&pattern, "")), ErrorKind::NestingTooDeep);
        assert_eq!(
            kind(Regex::xpath20(&pattern, "")),
            ErrorKind::NestingTooDeep
        );
    }
    assert!(Regex::xpath(&groups(64), "").is_ok());
}

#[test]
fn unknown_block() {
    for pattern in [
        r"\p{IsNoSuchBlock}",
        r"\P{IsNoSuchBlock}",
        r"[a\p{IsNoSuchBlock}]",
        r"[a-z-[\P{IsNo-Such-Block9}]]",
        r"a|\p{IsGreekAndCopticX}+",
    ] {
        for result in [
            Regex::xsd(pattern, ""),
            Regex::xpath(pattern, ""),
            Regex::xpath20(pattern, ""),
        ] {
            let error = result.unwrap_err();
            assert_eq!(error.kind(), ErrorKind::UnknownBlock, "{pattern:?}");
            assert!(matches!(error, Error::Syntax(_)));
        }
        // the kind says that `;k` would accept the pattern
        assert!(Regex::xsd(pattern, ";k").is_ok(), "{pattern:?}");
    }
    assert!(Regex::xsd(r"\p{IsBasicLatin}", "").is_ok());
}

#[test]
fn the_operations() {
    let empty = Regex::xpath("a?", "").unwrap();
    assert_eq!(
        empty.replace_all("b", "x").unwrap_err().kind(),
        ErrorKind::MatchesEmptyString
    );
    assert_eq!(
        empty.tokenize("b").unwrap_err().kind(),
        ErrorKind::MatchesEmptyString
    );
    assert_eq!(
        empty.analyze("b").unwrap_err().kind(),
        ErrorKind::MatchesEmptyString
    );
    let regex = Regex::xpath("a", "").unwrap();
    for replacement in [r"\x", "\\", "$", "$x"] {
        assert_eq!(
            regex.replace_all("a", replacement).unwrap_err().kind(),
            ErrorKind::InvalidReplacement,
            "{replacement:?}"
        );
    }
}

#[test]
fn every_variant_has_a_kind() {
    // the variants are those of regexml 0.2.2: matched exhaustively, so that
    // a new one fails to compile here
    let all = [
        Error::Internal,
        Error::InvalidFlags("x".to_string()),
        Error::Syntax("anything".to_string()),
        Error::MatchesEmptyString,
        Error::InvalidReplacementString("x".to_string()),
    ];
    let mut kinds = HashSet::new();
    for error in &all {
        let expected = match error {
            Error::Internal => ErrorKind::Internal,
            Error::InvalidFlags(_) => ErrorKind::InvalidFlags,
            Error::Syntax(_) => ErrorKind::Syntax,
            Error::MatchesEmptyString => ErrorKind::MatchesEmptyString,
            Error::InvalidReplacementString(_) => ErrorKind::InvalidReplacement,
        };
        assert_eq!(error.kind(), expected);
        kinds.insert(error.kind());
    }
    assert_eq!(kinds.len(), all.len());
}

#[test]
fn errors_display_their_message() {
    let error = Regex::xpath("(a", "").unwrap_err();
    let Error::Syntax(message) = &error else {
        panic!("{error:?}")
    };
    assert!(error.to_string().contains(message.as_str()), "{error}");
    let error = Regex::xpath("a", "z").unwrap_err();
    assert!(error.to_string().contains("'z'"), "{error}");
    assert!(!Error::MatchesEmptyString.to_string().is_empty());
    assert!(!Error::Internal.to_string().is_empty());
    fn is_std_error<E: std::error::Error + Send + Sync + 'static>(_: &E) {}
    is_std_error(&error);
}
