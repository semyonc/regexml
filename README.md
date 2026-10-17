# regexml-fork

> **An unofficial, maintained fork of [regexml](https://github.com/Paligo/regexml).**
> It is not affiliated with, or endorsed by, Paligo or the regexml maintainers.
> The crate is published as [`regexml-fork`](https://crates.io/crates/regexml-fork);
> the original remains [`regexml`](https://crates.io/crates/regexml).

This Rust workspace contains three related projects, as in the original repository:

- [regexml](regexml/README.md) — published as **`regexml-fork`**: a regular-expression engine that implements the
  regex languages of XML Schema 1.1 (Part 2, appendix G) and of the XPath and XQuery functions (`fn:matches`,
  `fn:replace`, `fn:tokenize`, `fn:analyze-string`).
- [regexml-convert-perl-tests](regexml-convert-perl-tests/README.md) — converter scripts for particular XPath tests.
- [regexml-ucd-blocks](regexml-ucd-blocks/README.md) — a script that maintains the `block.rs` module of the engine.

## Why this fork exists

[`xsd-schema`](https://github.com/semyonc/xsd-schema) ([crates.io](https://crates.io/crates/xsd-schema)), an XML
Schema 1.0/1.1 validator with an XPath 2.0 engine, uses this engine for XSD 1.1 `pattern` facets and for the XPath
regular-expression functions. Testing it against the W3C suites turned up defects and missing options in the engine:
wrong captured groups after backtracking, category sets rebuilt on every compile, a process abort on deeply nested
patterns, a hang in some reluctant repetitions, and no way to accept XSD 1.1's unknown block names.

Fixes for these were submitted upstream as [Paligo/regexml#16](https://github.com/Paligo/regexml/pull/16) on
2026-10-10, seven commits; it has had no response. An earlier pull request by another contributor,
[#15](https://github.com/Paligo/regexml/pull/15) (2026-08-21, whole-string matching), has had none either, and
issue [#12](https://github.com/Paligo/regexml/issues/12) (anchoring in XML Schema mode, opened 2025-05-12) has had
no fix or further reply since a maintainer welcomed tests and fixes on the day it was opened. The upstream
repository has had no new commits since its 0.2.2 release on 2026-04-28. A crate on crates.io can only depend on
crates published there, so the fixes can reach `xsd-schema`'s users only through a published crate. Hence this fork.

Beyond #16, and #15, which its author submitted, the changes made here have not been submitted upstream. They are
available under the same licenses as regexml, so regexml can take any of them as they are. If regexml resumes and releases these changes, this fork will
be deprecated in favour of regexml.

## What is different from regexml 0.2.2

See [`regexml/CHANGELOG.md`](regexml/CHANGELOG.md) for the details and the spec text each change relies on.

- **Captured groups are restored when the matcher backtracks** (F&O 3.1 §5.6.1: "only the last substring that it
  matched will be captured"). Results of `fn:replace`'s `$N`, `analyze-string` groups and back-references change
  where they were wrong.
- **Unicode category sets are built once** per process, not on every compile (`\p{Lu}+`: 65 µs → 2.7 µs).
- **Groups and character classes nested more than 64 levels deep** are a syntax error instead of a stack overflow.
- **The `;k` flag** makes an unknown `\p{IsX}` block match every character, as XML Schema 1.1 Part 2 §G.4.2.4 requires.
- **Reluctant repetitions that cannot reach their minimum no longer loop forever**, and a reluctant repetition that
  matches zero times leaves its groups unmatched.
- A lifetime made explicit on `Regex::matcher` (a compiler warning).
- **The `x` flag removes exactly #x9, #xA, #xD and #x20** (F&O 3.1 §5.6.2), no longer the form feed #xC as well.
- **Whole-string matching**, `Regex::is_full_match`: the matching operation of an XML Schema `pattern` facet, whose
  regular expressions are implicitly anchored "at the head and tail" (XML Schema 1.1 Part 2 appendix G). By
  [helping-kaiser](https://github.com/helping-kaiser), from Paligo/regexml#15, with more tests added here.
- **An XPath 2.0 dialect**, `Regex::xpath20`: the regular expressions of XQuery 1.0 and XPath 2.0 Functions and
  Operators (Second Edition) §7.6.1, which have no non-capturing groups `(?:…)` and only the flags `s`, `m`, `i` and
  `x` (§7.6.1.1: "If there are characters present that are not defined here as flags, then an error is raised"), and
  in which `.` without the `s` flag "matches any character except a newline (#x0A) character" — F&O 3.1 §5.6.2
  excludes the carriage return #xD as well, and `Regex::xpath` keeps that rule.
- **Error kinds**, `Error::kind`: a `#[non_exhaustive]` `ErrorKind` for every error — invalid flags (FORX0001), a
  syntax error (FORX0002), the zero-length match (FORX0003), an invalid replacement string (FORX0004) — and two
  syntax errors a caller may want to treat apart: groups and classes nested more than 64 levels deep, and an unknown
  block name that the `;k` flag would accept (XML Schema 1.1 Part 2 §G.4.2.4). `Error` also implements `Display`
  and `std::error::Error`. The `Error` variants are those of 0.2.2.

## Licenses and notices

The fork is distributed under the same terms as regexml: **Apache License 2.0 or Mozilla Public License 2.0**, at
your option ([`LICENSE-APACHE`](LICENSE-APACHE), [`LICENSE-MPL2`](LICENSE-MPL2)).

- The engine is a port of the regular-expression engine in **Saxon** (Copyright © 2018–2024 Saxonica Limited,
  MPL-2.0, "Incompatible With Secondary Licenses"), which itself extends **Apache Jakarta Regexp** (Apache-2.0). See
  [`COPYRIGHT`](COPYRIGHT) and [`CREDITS.txt`](CREDITS.txt), kept unchanged from regexml. Files derived from the
  Saxon code remain subject to the MPL-2.0; their source is this repository and the published crate.
- Third-party material keeps its own terms, also listed in `COPYRIGHT`: the Unicode `Blocks.txt` data (Unicode
  License V3) and the tests ported from the W3C qt3 test suite (W3C Software and Document License).
- **This is a modified version of regexml.** Files changed by the fork are identified by this repository's git
  history (every change since the fork point `b7af9de`, regexml v0.2.2) and summarised in the CHANGELOG.
- The name "regexml" is used only to identify the project this fork derives from; neither license grants any right
  to it, and no endorsement is implied.
- Contributions to this fork are accepted under the same dual license, so that regexml can take them as they are.

## AI disclosure

The fork's own changes were developed with AI assistance (Claude, Anthropic), then reviewed and accepted by the
maintainer; the commit that adds `Regex::is_full_match` is its author's, taken from Paligo/regexml#15. As with
`xsd-schema` (see its [AI Disclosure](https://github.com/semyonc/xsd-schema#ai-disclosure)), each change was checked
against external test suites rather than taken on trust: the engine's own tests, XQTS 1.0.2 and the W3C XML Schema
test suite through `xsd-schema`, the XSLT 3.0 test suite's 2.0 cases through an XSLT 2.0 processor, and a randomized
comparison against Python's `re`.
