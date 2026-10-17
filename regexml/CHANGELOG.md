# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## 0.3.0 — `regexml-fork`, unreleased

The first release of **`regexml-fork`**, an unofficial fork of regexml 0.2.2 (not affiliated with or endorsed by
Paligo). The library is `regexml_fork`; a project can keep the name `regexml` in its code with
`regexml = { package = "regexml-fork", version = "0.3" }`. The versions below 0.3.0 are regexml's own history, kept
as it was. The first seven changes were submitted upstream as Paligo/regexml#16; `is_full_match` is
Paligo/regexml#15, by its author. The minimum supported Rust version is 1.85 (`rust-version`; the code itself needs
1.79, for an inline `const` block).

### Changed (behaviour that was wrong in 0.2.2)

- **Captured groups are restored when the matcher backtracks.** An attempt that was later abandoned (an
  alternative, a repetition given up, a repetition count backed off from) left its captures behind, so a group lost
  what it had captured on the path that matched, and a group whose attempts all failed was reported as an empty
  match: `((a)|(ab))+c` on "aabc" gave group 2 = "" instead of "a". F&O 3.1 §5.6.1: "When a capturing
  sub-expression is matched more than once (because it is within a construct that allows repetition), then only the
  last substring that it matched will be captured." The matcher now logs every capture write with the value it
  replaces and undoes the log on backtracking; matching with groups is 20–33% faster on typical `analyze` and
  `replace_all` work. Two rules that held only by accident are now explicit: a back-reference to a group that has
  captured nothing matches the zero-length string (F&O 3.1 §5.6.1: "If no string has been matched by the Nth
  capturing sub-expression, the back-reference is interpreted as matching a zero-length string"), and a repetition
  that matches zero times leaves its operand's groups unmatched, so `^((.)?a\2)+$` matches "babadad" as the W3C
  test suite expects (fn-matches-50; W3C bug 29253). `([0-9]{1,2})?` no longer reports an empty group 1 for "1949":
  the group does not match there. (`80d245d`)
- **A reluctant repetition that matches zero times leaves its groups unmatched**, as the greedy one does:
  `^((a|bc)??x\2)+$` matches "bcxbcx" again. (`4f6d3d1`)
- **Reluctant repetitions that cannot reach their minimum no longer loop forever**: `(a|bc)+?` on "x" and
  `(a|bc){2,3}?` on "xa" used to hang `is_match`, `replace_all`, `tokenize` and `analyze`. The structure of Saxon's
  `OpRepeat` is restored. In a randomized comparison with Python's `re` (36 000 cases) all 754 hangs are gone and no
  case that agreed before differs. (`8275871`)
- **Groups and character classes nested more than 64 levels deep are a syntax error** (`Error::Syntax`, "Groups and
  character classes nested more than 64 levels deep") instead of a stack overflow that aborted the process.
  Compiling and matching recurse once per level; at the limit a debug build needs less than 400 KB of stack to
  compile and 150 KB to match. The deepest expressions in the W3C XML Schema test suite nest 15 levels. (`1b73605`)
- **A counted repetition keeps its upper bound when its operand can match the empty string.** XSD 1.1 Part 2 §G.2:
  S{n,m} denotes "All strings in L(S)^k for all k such that n <= k <= m". The compiler, as the Java original, turned
  `{n,m}` of such an operand into `*`, so `^(a|){2}$` matched "aaaa" and an XSD pattern facet like `(\d|){2}`
  accepted any number of digits. The bounds are now kept as written (empty repetitions count towards the minimum, see
  the next entry). And a repetition of an operand that only matches the empty string (an anchor or an empty group)
  with a minimum of one or more is the operand once instead of nothing: `a($)+b` no longer matches "ab". (`771fb03`,
  `45a0c79`)
- **A repetition of variable length tries every way of matching before it fails.** F&O 3.1 §5.6.1.2: a quantifier
  "matches the longest possible substring" (greedy) or "the shortest possible substring consistent with the match as
  a whole succeeding" (reluctant), so every way of matching is tried before there is no match. The iterators of the
  Java original did not: a counter (`ForceProgressIterator`) gave up after four results at the same position, so
  `^([ab]*)+b$` rejected "ab" and `^(.*)+B` rejected "AB" (Saxon bug 6426, fixed in Saxon 12.5 by raising the
  counter); the zero-repetitions alternative of a repetition was offered only once per position in a whole match
  (`History`), so `^(?:(a)b|a(b))(x|xy)*\2$` rejected "abb"; a reluctant repetition kept one way of matching each
  repetition, so `(a|ab)+?c` found nothing in "abc"; up to `min` repetitions could not all be empty, so
  `^($|a){3}$` rejected ""; and the possessive form of a repetition (`UnambiguousRepeat`) was used before an operation
  that can match the empty string or before `^`, so `^a*(?:(?:xy|x)*|z)a$` rejected "aa". The repetition is now a
  depth-first search over (repetitions, position) states, in the order the quantifier asks for. Up to the minimum a
  repetition may match the empty string; beyond it an empty repetition is the last one, which ends the search
  without losing a match. A state already explored is not explored again, and a repetition's results from a
  position are kept for the rest of the match. To keep every search polynomial and of the same order as regexml
  0.2.2's: with no upper bound all counts from the minimum on are one state; a repetition not nested in another one
  keeps, across the start positions of a search, the states and positions it settled (everything they led to was
  rejected); after the first repetition of an operand closed under repetition (a repetition with no upper bound,
  also in a group) the states need no successors, since the first repetition reached all they could (for greedy
  operands also where captures and the order of matches count, with the empty last repetition's captures made
  directly); and with back-references a state is told apart from another at its position only by the captures it
  passes on (back-referenced groups inside the operand that a repetition does not always write). Exponential searches
  became polynomial (optimized build, against 0.2.2): `(a+)+b`, `(a|aa)*b`, `(\w+\s?)*!`, `(a*?)*b` time out on 0.2.2
  at 100 characters and take 0.1–530 ms at 3 200; the rest of the family keeps 0.2.2's order — linear for a
  whole-string match, quadratic for a search over every start position — at 0.24 to 2.6 times 0.2.2's time at 3 200
  characters (`(a*)*b` searched in 3 200 a's, "c", "b": 0.38 s on 0.2.2, 0.18 s here). The exception is a
  back-reference to a group in such a repetition (see the known limitations). A whole-string match now checks, as a
  search does, the input's length and what every match needs before it tries the start. (`45a0c79`, the search's
  order of growth in the round-3 commit)
- **What follows `^` in multi-line mode is not tied to the start of the string.** The checks a search makes before
  it tries a start position (the operations every match needs) placed an operation after `^` at position 0 even in
  multi-line mode, where `^` also matches after a newline (F&O 3.1 §5.6.2: "^ matches the start of any line"):
  `.*\n^b` with `m` did not match "x\nb". (`2215c8e`)
- **A reluctant repetition of an operand that only matches the empty string finishes.** `()+?`, `(){2}?`, `($)+?`
  compiled to a fixed-length reluctant repetition of length 0, which offered the same position again for every
  further repetition, up to `usize::MAX` of them for `+?`, whenever what followed failed: `^(?:()+?)$` never finished
  on "a". It is now the operand once (or nothing, with a minimum of zero), as in the greedy case. (`6b885ce`)
- **A reluctant quantifier on `^` or `$` compiles.** F&O 3.1 §5.6.1.1 makes `^` and `$` character classes, so they
  take quantifiers; when the quantifier allowed zero occurrences (`$??`, `^*?`, `(?:$){0,}?`) the compiler left the
  `?` that makes it reluctant in the pattern, where it was read as a quantifier with nothing before it, a syntax
  error. (`fc71c20`)
- **The `x` flag no longer removes a form feed (#xC).** F&O 3.1 §5.6.2: "whitespace characters (#x9, #xA, #xD and
  #x20) in the regular expression are removed prior to matching with one exception: whitespace characters within
  character class expressions (charClassExpr) are not removed" (F&O 1.0 §7.6.1.1 says the same). The engine removed
  every ASCII whitespace character, #xC included, so under `x` a form feed in the pattern disappeared and, between `(`
  and `?`, turned a capturing group into a non-capturing one. Now exactly the four are removed, in every dialect.
  (`17463ff`)

### Added

- **The `;k` flag for unknown block names.** With the flags ending in `;k`, `\p{IsX}` and `\P{IsX}` with an
  unrecognized block name X denote all characters, in both dialects, in and outside character classes. XML Schema
  1.1 Part 2 §G.4.2.4: "If a string "IsX" matches the non-terminal IsBlock but X is not a recognized block name, then
  the expressions "\p{IsX}" and "\P{IsX}" each denote the set of all characters." A name that does not match
  IsBlock stays an error, and so does any unknown name without the flag (F&O 3.1 §5.6.1.5). Saxon's flag was parsed
  but unused. (`9226461`)
- **`Regex::is_full_match`**, whole-string matching: the matching operation of XML Schema's `pattern` facet, whose
  regular expressions are implicitly anchored (XML Schema 1.1 Part 2 appendix G: "the regular expression language
  defined here implicitly anchors all regular expressions at the head and tail"). `is_match` searches. Every way the
  expression can match from the start is tried until one ends at the end of the string, so `a|ab` full-matches
  "ab". In the XSD dialect `^` and `$` are ordinary characters (§G.3). By helping-kaiser, from Paligo/regexml#15
  (`f669c2d`); more tests, and no capture tracking without back-references (`c630612`).
- **`Regex::xpath20`**, the XPath 2.0 dialect of XQuery 1.0 and XPath 2.0 Functions and Operators (Second Edition)
  §7.6.1: the XPath dialect without what F&O 3.0 added. Only the flags `s`, `m`, `i` and `x` (§7.6.1.1: "If there
  are characters present that are not defined here as flags, then an error is raised [err:FORX0001]"), so `q` and
  `;…` are `Error::InvalidFlags`; no non-capturing groups, so an unescaped `(` followed by `?` outside a class is an
  `Error::Syntax`; and without `s`, `.` "matches any character except a newline (#x0A) character" (§7.6.1.1), where
  F&O 3.1 §5.6.2 also excludes the carriage return #xD. `x` removes exactly "whitespace characters (#x9, #xA, #xD and
  #x20)". `Regex::xpath` is unchanged. (`638ddd6`)
- **`Error::kind`** and **`ErrorKind`** (`#[non_exhaustive]`): `InvalidFlags` (FORX0001), `Syntax` (FORX0002),
  `NestingTooDeep`, `UnknownBlock`, `MatchesEmptyString` (FORX0003), `InvalidReplacement` (FORX0004), `Internal`. The
  nesting limit and an unknown block name are `Error::Syntax` as before, with kinds of their own: the first may be
  reported as an implementation limit, the second is what `;k` accepts (§G.4.2.4). `Error` implements `Display` and
  `std::error::Error`. The `Error` variants and messages are unchanged. (`ba58724`)

### Performance

- **Unicode category sets are built once per process**, not on every compile: a `OnceLock` per general category and
  one for `\w`; `\d` uses the cached `\p{Nd}`. Compile time, optimized build: `\p{Lu}+` 65 → 2.7 µs,
  `\d{3}-\w+` 401 → 5.0 µs, `\w+@\w+\.\w+` 881 → 12.6 µs, `[\p{L}-[\p{Lu}]]` 220 → 81 µs (the subtraction
  remains). `Regex` stays `Send + Sync`. (`6d881ef`)

### Other

- The lifetime of the matcher `Regex::matcher` returns is spelled out (`ReMatcher<'_>`; the
  `mismatched_lifetime_syntaxes` warning). (`5c028bf`)
- The crate-private `Language::XSD` is `Language::Xsd` (clippy `upper_case_acronyms`). (`3fc4ec1`)
- `rust-version = "1.85"`; the two tool crates of the workspace are `publish = false`. (`017f0d6`)
- Package metadata, README and licence notices of the fork. (`bf48f9f`)
- The package carries `LICENSE-APACHE`, `LICENSE-MPL2`, `COPYRIGHT` and `CREDITS.txt`, byte-identical copies of the
  files at the repository root; regexml 0.2.2's package carried none of them. (`d95e9a2`)

### Known limitations

- A back-reference to a group inside a repetition whose operand matches many lengths, as in `(a*)*\1b`, makes the
  search tell apart every substring the group can have captured: quadratic work for each start position, a little
  more with the comparisons (3 200 characters: 3.3 s for a whole-string match, 4.5 s for a search), where regexml
  0.2.2 cut the search short (0.25 s / 0.4 s, with wrong answers elsewhere, as for `^(?:(a)b|a(b))(x|xy)*\2$`).
- Character class subtraction and union are quadratic in ICU's set builder.

## [0.2.2](https://github.com/Paligo/regexml/compare/regexml-v0.2.1...regexml-v0.2.2) - 2026-04-28

### Fixed

- fix panic in analyze-string

## [0.2.1](https://github.com/Paligo/regexml/compare/regexml-v0.2.0...regexml-v0.2.1) - 2025-03-19

- Improve cargo metadata.

## [0.2.0](https://github.com/Paligo/regexml/compare/regexml-v0.1.1...regexml-v0.2.0) - 2025-03-19

- Implement API docs.

### Breaking

- The `Language` enum was removed from the public API as it was not in use.

## [0.1.1](https://github.com/Paligo/regexml/compare/regexml-v0.1.0...regexml-v0.1.1) - 2025-03-19

### Other

- Fix license.
- Update license fields.

## [0.1.0](https://github.com/Paligo/regexml/releases/tag/regexml-v0.1.0) - 2025-03-19

Initial public release.