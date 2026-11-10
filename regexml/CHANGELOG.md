# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## 0.4.0 — `regexml-fork`, unreleased

The public API is unchanged from 0.3.0: no item is added or removed. This is 0.4.0 and not 0.3.1 because the minimum
supported Rust version rises from 1.85 to 1.88 and the character data changes with ICU4X 2.4 (Unicode 18.0), so a
project that depends on `regexml-fork = "0.3"` keeps 0.3.0 until it opts in. Besides those, three wrong answers are
fixed, and `is_match` and `is_full_match` are faster.

### Changed (behaviour that was wrong in 0.3.0)

- **Under the `i` flag, a repetition followed by a literal whose first character has no case variants no longer
  rejects matches.** Everyday case-blind expressions matched nothing: `.*\.xml` on "file.XML" (and on "file.xml"),
  `.*1` on "a1", `\d*5` on "55", `[a-z\-]*-1` on "a-1", `\S+@\S+` on "a@b", `.+,.+` on "a,b", `.*/` on "a/b/",
  `\s*\(.*\)` on " (x)" were all false for `is_match` and `is_full_match` in 0.3.0, and `replace_all` left the input
  unchanged; so did `tokenize` and `analyze`. The trigger is any repetition followed by a literal whose first
  character has an ICU case closure disjoint from the repeated class, which is always so for a character without
  case variants (a digit, a punctuation mark), whose closure is empty; literals with case variants were caught too,
  `a*A` on "aA", `a?A` on "A", `i*İ` on "İ" (U+0130). The compiler makes a repetition possessive (it keeps as many
  repetitions as it can, never fewer) when what follows cannot start with a character the repetition can match, and
  judged that from the ICU case closure of the literal's first character: but a closure leaves out the character
  itself ('a' gives {'A'}, '.' gives nothing), and literals compare under `i` by their simple lowercase mappings, by
  which 'i' matches 'İ' although neither is in the other's closure. So `.*` took the ".xml" that the literal after it
  needed, and `a*A` the 'A'. The first character of a case-blind literal now counts as any character for that
  judgement, so such a repetition stays an ordinary one; character classes, whose case variants are added when they
  are compiled, are judged as before. Without `i` nothing changes.
- **The empty pattern with the `q` flag matches the empty string as a whole**: `Regex::xpath("", "q")
  .is_full_match("")` was false (and `is_match("")` true). The check that a literal the expression needs occurs in
  the input looked at every position but the end, where an empty literal occurs too.
- **Repetition counts near the largest `usize` no longer wrap around**: the lengths computed from them (the
  shortest match of an expression, a fixed-length repetition's furthest end) saturate. `..(ab){1,9223372036854775807}`
  rejected "zzab" in an optimized build, `abc(x){18446744073709551613}` panicked on "" (index out of bounds), and a
  debug build panicked when compiling such counts. On a 32-bit target `(ab){1,2147483647}` already wrapped.

### Changed

- **ICU4X 2.4** (`icu_casemap`, `icu_collections` and `icu_properties` 2.4 or later, was 1.5), **and with it the
  character data of Unicode 18.0 instead of 15.1.** The category escapes `\p{…}` and `\P{…}`, `\d`, `\w` and their
  complements, and case-blind matching under the `i` flag, see the characters assigned since 15.1: `\p{L}` holds
  158,172 code points instead of 136,726, `\p{Lu}` 1,906 instead of 1,831, `\p{Nd}` 770 instead of 680, and `\w`
  gains 22,977 characters, all of them unassigned in 15.1 (U+1C89 CYRILLIC CAPITAL LETTER TJE, for one, is now in
  `\p{Lu}`). Among the characters assigned in 15.1 exactly two change category: U+0295 LATIN LETTER PHARYNGEAL VOICED
  FRICATIVE from `Ll` to `Lo` (so `\p{Ll}` no longer matches it) and U+1171E, an Ahom consonant sign, from `Mn` to
  `Mc`. Case-blind matching gains pairs only for capitals added since 15.1 (U+019B with U+A7DC, U+0264 with U+A7CB,
  for example). `\s`, `\i`, `\c` and `.` do not change, nor do the block escapes (`\p{IsBasicLatin}`), which come
  from the crate's own block table. The requirements are carets, so a project that already depends on ICU4X 2
  (through `url` and `idna`, for example) now builds one copy of it instead of two, and `zerovec` 0.10, which the 1.5
  crates brought, is no longer in the dependency graph. **The minimum supported Rust version is 1.88**, which ICU4X
  2.4 needs: with Cargo's default resolver every ICU4X 2 requirement takes 2.4 (even `~2.2.0` brings `icu_provider`
  and `icu_locale_core` 2.4, which the 2.2 crates require with a caret), so no lower minimum would build without
  resolving with `rust-version` in mind, and with 2.4 as the floor the character data is that of Unicode 18.0 in
  every build.

### Performance

- **`is_match` and `is_full_match` answer from sets of positions, without backtracking and without allocating, for
  haystacks of up to 127 characters.** When only the answer counts and the expression has no back-reference and no
  `^` or `$` (always so in the XSD dialect, where they are ordinary characters), whether the haystack matches is
  whether it is in the language of the expression (XML Schema 1.1 Part 2 appendix G: S{n,m} denotes "all
  concatenations of at least n, and at most m, strings from L(S)"), whatever order the matcher would try the
  alternatives and repetition counts in. For each operation, the evaluator computes the positions where a match of
  it can end from a set of positions where it starts, a `u128` with a bit per position of the haystack, whose
  characters are copied to the stack. A repetition takes at most n + 2 steps for its minimum and n + 1 beyond it for
  a haystack of n characters, whatever its bounds (`(a?){5000}` included), and a repeated atom or character class is
  one mask and one shift per repetition. The mask is filled in as the repetition goes: each position is looked up
  once, when a repetition first starts from it, except those its minimum goes through, which are looked up in one
  pass; so a repetition costs the characters it reaches, and `\w+` inside an outer repetition does not look at the
  rest of the haystack at every outer step. Nested repetitions multiply their steps, so one answer may take at most
  2,048 evaluations of an operation (the NF-e date and time pattern below takes 112), after which the matcher
  answers: deeply nested repetitions such as `((((a){1,2}){1,2}){1,2})…` cost well under a millisecond before it
  takes over. As the matcher does, a haystack shorter than the shortest match is rejected at once. The possessive
  repetitions the compiler makes where it judges that fewer repetitions cannot lead to a match are computed as the
  matcher runs them, so the answers stay the matcher's; the case-blind comparison of characters (`i`) is one
  function that both use. Longer haystacks, back-references, and `^` and `$` in the XPath dialects, go to the
  matcher as before; whether an expression qualifies is decided once, when the `Regex` is made. A differential test
  generates 112,339 random expression and haystack pairs, of which 106,828 are answered by the sets of positions
  (2.50 and 2.37 million in an optimized run), compares the two answers, and compares both with an evaluator of the
  generated expressions themselves, which is how the first of the fixes above was found: no difference. On 21
  values of the `pattern` facets of the Brazilian NF-e schema, optimized build, per `is_full_match`: 899 → 107 ns on
  average and 30 → 0 allocations; `[0-9]{44}` on a 44-digit access key 3.9 µs, 115 allocations and 14 KB → 0.20 µs
  and none; the `TDateTimeUTC` pattern 2.6 → 0.51 µs. Lists of words or numbers, where the sets of positions gain
  least, still gain: `(\w+\s?)+` on 126 characters of one-letter words 14 → 6.4 µs, `((\+|-)?\d+(\.\d+)?\s*)+` on
  115 characters of numbers 17 → 6.1 µs. Validating a 762 KB NF-e document against that schema with the
  `xsd-schema` crate (streaming) takes 50.7 → 32.6 ms, with 945,398 → 278,404 allocations.

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
  passes on (back-referenced groups inside the operand that a repetition does not always write). Where only the
  answer counts (`is_match`, `is_full_match`), a fixed-length repetition in the operand of a repetition that is not
  nested (with only sequences, choices and groups in between) stops where it has been before with as many
  repetitions, since what follows it is the same each time: without that, `(a*b*)*c` over a run of a's it cannot
  match through went through the rest of the run again from every position in it. Exponential searches became
  polynomial (optimized build, against 0.2.2): `(a+)+b`, `(a|aa)*b`, `(\w+\s?)*!`, `(a*?)*b` and the others that
  time out on 0.2.2 at 100 or 200 characters take 0.1 ms to 1.2 s at 3 200. The rest of the family grows no faster
  than on 0.2.2 — linear for a whole-string match and, where only the answer counts, for a search too (but for a
  repetition nested in the operand, as in `((a*)*)*b`: quadratic, as on 0.2.2); quadratic for `replace_all` and
  `tokenize` over every start position — at 0.001 to 2.0 times 0.2.2's time at 3 200 characters (`(a*)*b` searched
  in 3 200 a's, "c", "b": 0.39 s on 0.2.2, 1.9 ms here; `(a*b*)*c` matched as a whole against 3 200 a's and "cb":
  0.67 ms on 0.2.2 and here). The exceptions are listed under "Known limitations". A whole-string match now
  checks, as a search does, the input's length and what every match needs before it tries the start. (`45a0c79`,
  `d9d7811`, `eb603cb`, `95acd7f`)
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
  search tell apart every substring the group can have captured: over 3 200 a's and "cb", 3.1 s for a whole-string
  match and 4.8 s for a search, against 0.25 ms and 0.40 s on regexml 0.2.2, whose speed there comes from the
  counter that cut the search short and gave wrong answers elsewhere (`^(?:(a)b|a(b))(x|xy)*\2$` rejected "abb").
  XSD patterns have no back-references, so only the XPath functions meet it.
- `replace_all`, `tokenize` and `analyze`, which keep captures and the order of matches, take time quadratic in the
  length of a run that an expression which can only match at the start cannot match through, when a repetition in
  it has an operand that is not closed under repetition but has a repetition inside: `^(?:(a*b*)*c)$` over 3 200
  a's and "cb", 0.34 s against 0.67 ms on 0.2.2, whose counter gave up after four results at the same position
  (`is_match` and `is_full_match` take 0.6 ms). Without the `^`, over every start position, both are quadratic.
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