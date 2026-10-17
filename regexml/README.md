# regexml-fork

> **An unofficial, maintained fork of [`regexml`](https://crates.io/crates/regexml)**
> ([Paligo/regexml](https://github.com/Paligo/regexml)), not affiliated with or endorsed by its maintainers.
> Why it exists, what changed and the licensing details:
> [the repository README](https://github.com/semyonc/regexml#readme).

To start off, don't worry. We're not using regexes to parse XML in this crate.
We're not in fact dealing with XML directly at all.

If you need a Regex engine in Rust, this crate isn't likely for you; use the
[regex crate](https://crates.io/crates/regex) instead. This crate instead
implements a Regex engine compliant with various XML-related standards, and
focuses on standard-compliance rather than performance.

`regexml-fork` implements a regular expression engine that's compliant with regular
expressions as defined in appendix G of the _XML Schema 1.1 standard, part 2_:

<https://www.w3.org/TR/xmlschema11-2/#regexs>

This is the regex language that XML Schema uses so the user can define patterns
as additional constraints on string data in an XML document:

<https://www.w3.org/TR/xmlschema11-2/#dc-pattern>

The _XPath and XQuery Functions and Operators 3.1_ specification defines an
extension of these regular expressions for the purposes of use within the XPath
and XQuery standard function library:

<https://www.w3.org/TR/xpath-functions-31/#regex-syntax>

`regexml-fork` also implements this extension.

## Using it

The API is that of `regexml` 0.2.2, with additions. A project that already uses `regexml` can switch without
touching its code by renaming the dependency:

```toml
[dependencies]
regexml = { package = "regexml-fork", version = "0.3" }
```

or use the crate under its own name:

```rust
use regexml_fork::Regex;

// The XPath dialect, as used by fn:matches: a match anywhere in the string.
let re = Regex::xpath(r"\p{Lu}+", "").unwrap();
assert!(re.is_match("aBCd"));

// The XML Schema dialect, matched as a pattern facet matches: the whole string.
let re = Regex::xsd(r"[A-Z]{2}\d+", "").unwrap();
assert!(re.is_full_match("AB12"));
assert!(!re.is_full_match("AB12x"));

// `;k` accepts unknown block names (XSD 1.1 Part 2 §G.4.2.4).
assert!(Regex::xsd(r"\p{IsNoSuchBlock}", ";k").is_ok());
```

Behaviour differs from `regexml` 0.2.2 only where 0.2.2 was wrong: captured groups after backtracking, patterns
nested more than 64 levels deep (now a syntax error), unknown block names with `;k`, and reluctant repetitions that
used to loop. The additions change nothing that was there before: `Regex::is_full_match` (whole-string matching,
contributed by [helping-kaiser](https://github.com/helping-kaiser) in Paligo/regexml#15)<!-- draft: update when
F2/F3 land -->, `Regex::xpath20` (the XPath 2.0 dialect of F&O 1.0 §7.6.1: no `(?:…)`, the flags `s`, `m`, `i`
and `x` only, and `.` excluding only #xA) and `Error::kind` (a classification of errors to match on instead of
their text)<!-- end draft -->. The
[CHANGELOG](https://github.com/semyonc/regexml/blob/fork/regexml/CHANGELOG.md) lists each change.

## Origins

The Rust source code is based on the Java implementation in Saxon HE
`net.sf.saxon.regex`, which implements a spec-compliant Regex engine. In turn
this code is based on an engine implemented by Apache Jakarta:

<https://blog.saxonica.com/mike/2012/01/a-new-regex-engine.html>

The Java code was translated by hand into Rust by the regexml project. There are some differences:

- `Operation` is an enum, instead of implemented using subclasses and dynamic
  dispatch as in the Java version. A trait `OperationControl` provides dispatch
  to the enums.

- The [`icu4x` project](https://docs.rs/icu/latest/icu/)'s `icu_` crates are
  used to provide various unicode features, including the implementation of
  character classes and casing rules. Especially `CodePointInversionList` and
  its associated builder proved very useful. Due to the way the regex compiler is
  organized `CharacterClass` does provide a special case for character class
  that matches with a single character.

- The original code had no internal tests, but a lot of integration tests were
  provided through the [qt3tests](https://github.com/w3c/qt3tests) project for
  testing XPath and XQuery. Most of those tests have been ported into simple
  Rust tests, which makes this package easier to maintain and debug.

## License

Apache License 2.0 **or** Mozilla Public License 2.0, at your option — the same terms as `regexml`. The engine is a
port of Saxon's regular-expression engine (Copyright © 2018–2024 Saxonica Limited, MPL-2.0), which extends Apache
Jakarta Regexp (Apache-2.0); files derived from the Saxon code remain subject to the MPL-2.0. This is a modified
version of `regexml`: the changes are recorded in the repository's history and CHANGELOG. The full notices — including
the Unicode data license and the W3C test-suite license — are in the
[`COPYRIGHT`](https://github.com/semyonc/regexml/blob/fork/COPYRIGHT) and
[`CREDITS.txt`](https://github.com/semyonc/regexml/blob/fork/CREDITS.txt) files, kept unchanged from `regexml`.
