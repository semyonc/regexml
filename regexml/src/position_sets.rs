//! `is_match` and `is_full_match` answered from sets of positions, without
//! backtracking and without allocating.
//!
//! # What is computed
//!
//! A position is a place between two characters of the haystack: 0 before
//! the first, n after the last, for a haystack of n characters. For an
//! operation `op` and a set S of positions, `ends(op, S)` is the set of
//! positions where a match of `op` that starts at a position in S can end.
//!
//! When only a yes or no is asked and the expression has no back-reference
//! and no `^` or `$`, the answer does not depend on which alternative or which
//! repetition count the backtracking matcher tries first, nor on what the
//! groups capture: it is whether the haystack (or a substring of it) is in the
//! language of the expression, the set of strings XSD 1.1 Part 2 Appendix G
//! calls L(R) and that F&O 3.1 §5.6.1 builds on. So:
//!
//! - `is_full_match` (an XSD `pattern` facet: the language of Part 2
//!   Appendix G "implicitly anchors all regular expressions at the head and
//!   tail") is n in `ends(program, {0})`, where the end of the program accepts
//!   only n;
//! - `is_match` (`fn:matches`, true when some substring of the input matches,
//!   F&O 3.1 §5.6.3) is `ends(program, {0, ..., n})` not empty, where the end
//!   of the program accepts every position.
//!
//! A set is a `u128`, bit p for position p, so a haystack of at most
//! [`MAX_CHARS`] characters is answered here, from its characters copied to
//! the stack; a longer one, or a program `supports` turns down, goes to the
//! matcher.
//!
//! # The operations
//!
//! - An atom of L characters moves p to p + L where the L characters from p
//!   are those of the atom; under the `i` flag compared as the matcher does
//!   (`re_matcher::equal_case_blind`).
//! - A character class moves p to p + 1 where the character at p is in the
//!   class. Under the `i` flag the compiler has already added the case
//!   variants to the class, and the matcher does not compare case-blind
//!   either.
//! - A sequence applies its operations in turn, a choice is the union of its
//!   branches, a group is its operand, "nothing" keeps the set.
//! - The end of the program: see above.
//! - The repetitions with a minimum and a maximum (`Repeat`, `GreedyFixed`,
//!   `ReluctantFixed`): greedy or reluctant, of fixed length or not, they all
//!   match "all concatenations of at least n, and at most m, strings from
//!   L(S)" (Part 2 §G.2), which is computed as described below.
//! - The possessive repetition (`UnambiguousRepeat`) repeats an atom or a
//!   class as many times as it can, up to its maximum, and gives only that
//!   end, or none if that is fewer than its minimum. The compiler uses it where
//!   it has judged that fewer repetitions could not lead to a match
//!   (`ReCompiler::no_ambiguity`, or a minimum equal to the maximum), and it
//!   is computed here as the matcher runs it, so that both give the same
//!   answer whether or not that judgement holds.
//!
//! # Repetitions
//!
//! Let R be one repetition of the operand: R(S) = `ends(operand, S)`. It is
//! distributive over union. For an operand that is an atom or a character
//! class, R(S) is one mask and one shift: (S & mask) << L, where the mask
//! holds the positions the operand matches from. It is filled in as the
//! repetition goes, each position looked up once, when a repetition first
//! starts from it; only the positions the minimum goes through (from the
//! first position in S to the last plus min - 1 lengths) are looked up at
//! once, in one pass, so that `[0-9]{44}` is one scan of the haystack and 44
//! masks and shifts. A repetition so costs the positions it reaches, not the
//! rest of the haystack: `\w+` inside an outer repetition, evaluated again at
//! each outer step, looks at the characters of one word each time, which
//! keeps `(\w+\s?)+` linear in the length of the haystack.
//!
//! 1. The minimum: S(k) = R(S(k - 1)) for k = 1, ..., min, with S(0) = S.
//!    If S(k) is empty there is no match. If S(k) = S(k - 1), every further
//!    S is the same, so S(min) = S(k). That happens soon: without anchors,
//!    an operand matches the empty string at every position or at none
//!    (whether the empty string is in its language). If it does, S(k) is
//!    contained in S(k + 1), so after at most n + 1 growing steps it stops
//!    changing; if it does not, every position moves right at each step, so
//!    after at most n + 1 steps the set is empty. So `{5000,}` takes at most
//!    n + 2 steps. (A possessive repetition, whose empty match does depend on
//!    the position, has a minimum of one or more, or the compiler makes it
//!    only before an operation that cannot match the empty string, so it is on
//!    no empty match of the operand. And the loop is exact in any case: it
//!    would only take more steps.)
//! 2. Beyond the minimum: the result is the union of S(min), ..., S(max).
//!    It is computed breadth-first: `reached` = `frontier` = S(min), then,
//!    while there are repetitions left, `frontier` = R(`frontier`) minus
//!    `reached`, stopping when it is empty, and `reached` gains it. Expanding
//!    only the new positions is enough: a position first reached after fewer
//!    repetitions has at least as many repetitions left as when it is
//!    reached again later, so what it reaches then it reached the first time.
//!    `reached` grows at each step, so this takes at most n + 1 steps
//!    whatever the maximum.
//!
//! A possessive repetition is computed with a mask filled in the same way,
//! one repetition at a time: from the minimum on, the positions where the
//! operand does not match again are ends (and all of them at the maximum).
//! Every position moves right at each step, so this too ends after at most
//! n + 1 steps.
//!
//! # The budget
//!
//! The steps of nested repetitions multiply: each step of a repetition
//! evaluates its operand, so d nested repetitions can take up to about
//! (n + 2)^d evaluations of the innermost one. (`(((a*)*)*)*` took twice as long
//! per level before the minimum's fixed point ended the repetition at once: the
//! compiler gives a repetition of an operand that matches the empty string a
//! minimum of one.) So one answer may take at most [`BUDGET`] evaluations of an
//! operation; past that, the matcher answers. Real expressions stay far below
//! it (the date and time pattern of the NF-e schema takes 112), and an
//! evaluation costs at most a scan of the positions, so an answer that runs
//! out of it costs well under a millisecond before the matcher takes over.
//!
//! Recursion follows the nesting of the expression, which the compiler limits
//! to 64 levels.

use std::cell::Cell;

use crate::character_class::CharacterClass;
use crate::operation::Operation;
use crate::re_matcher::equal_case_blind;
use crate::re_program::{ReProgram, OPT_HASBACKREFS};

/// The longest haystack, in characters, answered here: its positions 0 to
/// `MAX_CHARS` are the bits of a `u128`.
pub(crate) const MAX_CHARS: usize = 127;

/// A set of positions, bit p for position p.
type Positions = u128;

/// The most evaluations of an operation (`Haystack::ends`) one answer may
/// take before it is left to the matcher (see "The budget" above).
pub(crate) const BUDGET: u32 = 1 << 11;

#[cfg(test)]
thread_local! {
    // Turns the possessive repetition into an ordinary one (from its minimum
    // to its maximum), so that tests can tell where the compiler's choice of
    // it changes an answer.
    pub(crate) static POSSESSIVE_AS_COUNTED: Cell<bool> = const { Cell::new(false) };
    // How many times a position was looked up for an atom or a character
    // class, so that tests can tell what an answer costs.
    pub(crate) static LOOKUPS: Cell<usize> = const { Cell::new(0) };
}

/// Counts a lookup of an atom or a character class at one position.
#[inline(always)]
fn count_lookup() {
    #[cfg(test)]
    LOOKUPS.with(|lookups| lookups.set(lookups.get() + 1));
}

/// Whether `program` can be answered here, decided once per `Regex`: no
/// back-reference, no `^` or `$`, and possessive repetitions of a non-empty
/// atom or a character class only.
pub(crate) fn supports(program: &ReProgram) -> bool {
    program.optimization_flags & OPT_HASBACKREFS == 0 && supported(&program.operation)
}

fn supported(operation: &Operation) -> bool {
    match operation {
        Operation::Atom(_)
        | Operation::CharClass(_)
        | Operation::Nothing(_)
        | Operation::EndProgram(_) => true,
        Operation::Sequence(sequence) => sequence.operations.iter().all(supported),
        Operation::Choice(choice) => choice.branches.iter().all(supported),
        Operation::Capture(capture) => supported(&capture.child_op),
        Operation::Repeat(repeat) => supported(&repeat.operation),
        Operation::GreedyFixed(repeat) => repeat.len > 0 && supported(&repeat.operation),
        Operation::ReluctantFixed(repeat) => repeat.len > 0 && supported(&repeat.operation),
        Operation::UnambiguousRepeat(repeat) => match repeat.operation.as_ref() {
            Operation::Atom(atom) => !atom.atom.is_empty(),
            Operation::CharClass(_) => true,
            _ => false,
        },
        // `^` and `$` hold at positions that depend on lines and on where the
        // search starts; a back-reference depends on the captures.
        Operation::Bol(_) | Operation::Eol(_) | Operation::BackReference(_) => false,
    }
}

/// Whether the whole of `haystack` matches `program`, which `supports`
/// accepts; `None` if the haystack is longer than [`MAX_CHARS`].
pub(crate) fn is_full_match(program: &ReProgram, haystack: &str) -> Option<bool> {
    let mut buffer = ['\0'; MAX_CHARS];
    let chars = collect(haystack, &mut buffer)?;
    let n = chars.len();
    // as the matcher does first: a match is at least this long
    if n < program.minimum_length {
        return Some(false);
    }
    let haystack = Haystack::new(chars, program, true);
    let ends = haystack.ends(&program.operation, 1);
    haystack.within_budget().then_some((ends >> n) & 1 == 1)
}

/// Whether some substring of `haystack` matches `program`, which `supports`
/// accepts; `None` if the haystack is longer than [`MAX_CHARS`].
pub(crate) fn is_match(program: &ReProgram, haystack: &str) -> Option<bool> {
    let mut buffer = ['\0'; MAX_CHARS];
    let chars = collect(haystack, &mut buffer)?;
    if chars.len() < program.minimum_length {
        return Some(false);
    }
    let haystack = Haystack::new(chars, program, false);
    let ends = haystack.ends(&program.operation, below(chars.len() + 1));
    haystack.within_budget().then_some(ends != 0)
}

/// The characters of `haystack` in `buffer`, or `None` if there are more
/// than it holds.
fn collect<'b>(haystack: &str, buffer: &'b mut [char; MAX_CHARS]) -> Option<&'b [char]> {
    // a character is at most four bytes
    if haystack.len() > 4 * MAX_CHARS {
        return None;
    }
    let mut n = 0;
    for c in haystack.chars() {
        *buffer.get_mut(n)? = c;
        n += 1;
    }
    Some(&buffer[..n])
}

/// The positions below `k`: 0 to k - 1.
fn below(k: usize) -> Positions {
    if k >= Positions::BITS as usize {
        Positions::MAX
    } else {
        (1 << k) - 1
    }
}

/// Calls `f` with each position in `set`, in increasing order.
fn for_each(mut set: Positions, mut f: impl FnMut(usize)) {
    while set != 0 {
        f(set.trailing_zeros() as usize);
        set &= set - 1;
    }
}

/// The haystack, and how the program is matched against it.
struct Haystack<'h> {
    chars: &'h [char],
    case_blind: bool,
    // whether the end of the program accepts only the end of the haystack
    // (a whole-string match) or every position (a search)
    anchored: bool,
    // how many more evaluations of an operation this answer may take; at 0,
    // every evaluation gives the empty set and the answer is thrown away
    budget: Cell<u32>,
}

/// How one repetition of an operand moves a set of positions.
enum Step<'o> {
    /// An atom or a character class, which matches `len` characters exactly.
    /// `mask` (the "mask" of the module documentation) holds the positions it
    /// matches from, among those looked up so far (`known`): those the
    /// minimum needs at once (`Haystack::seed`), any other when a repetition
    /// first starts from it, so a repetition costs the positions it touches,
    /// not the rest of the haystack. One repetition is (set & mask) << len.
    Mask {
        operand: &'o Operation,
        len: usize,
        known: Positions,
        mask: Positions,
    },
    /// Any other operand: its ends.
    Ends(&'o Operation),
}

impl<'h> Haystack<'h> {
    fn new(chars: &'h [char], program: &ReProgram, anchored: bool) -> Self {
        Self {
            chars,
            case_blind: program.flags.is_case_independent(),
            anchored,
            budget: Cell::new(BUDGET),
        }
    }

    /// Whether the evaluations so far stayed within [`BUDGET`], so that their
    /// answer holds. (One that used it up exactly is thrown away as well.)
    fn within_budget(&self) -> bool {
        self.budget.get() > 0
    }

    /// The positions where a match of `operation` that starts at a position
    /// in `from` can end.
    fn ends(&self, operation: &Operation, from: Positions) -> Positions {
        if from == 0 {
            return 0;
        }
        let left = self.budget.get();
        if left == 0 {
            return 0;
        }
        self.budget.set(left - 1);
        match operation {
            Operation::Atom(atom) => self.atom_ends(&atom.atom, from),
            Operation::CharClass(class) => self.class_ends(&class.character_class, from),
            Operation::Sequence(sequence) => {
                let mut at = from;
                for operation in &sequence.operations {
                    at = self.ends(operation, at);
                    if at == 0 {
                        break;
                    }
                }
                at
            }
            Operation::Choice(choice) => choice
                .branches
                .iter()
                .fold(0, |ends, branch| ends | self.ends(branch, from)),
            Operation::Capture(capture) => self.ends(&capture.child_op, from),
            Operation::Nothing(_) => from,
            Operation::EndProgram(_) => {
                if self.anchored {
                    from & (1 << self.chars.len())
                } else {
                    from
                }
            }
            Operation::Repeat(repeat) => {
                self.counted(&repeat.operation, repeat.min, repeat.max, None, from)
            }
            Operation::GreedyFixed(repeat) => self.counted(
                &repeat.operation,
                repeat.min,
                repeat.max,
                Some(repeat.len),
                from,
            ),
            Operation::ReluctantFixed(repeat) => self.counted(
                &repeat.operation,
                repeat.min,
                repeat.max,
                Some(repeat.len),
                from,
            ),
            Operation::UnambiguousRepeat(repeat) => {
                self.possessive(&repeat.operation, repeat.min, repeat.max, from)
            }
            Operation::Bol(_) | Operation::Eol(_) | Operation::BackReference(_) => {
                unreachable!("turned down by `supports`")
            }
        }
    }

    /// From `min` to `max` repetitions of `operand` (`max` is `usize::MAX`
    /// for no upper bound); see the module documentation. `len` is the
    /// length of every match of the operand, when the compiler made a
    /// fixed-length repetition of it.
    fn counted(
        &self,
        operand: &Operation,
        min: usize,
        max: usize,
        len: Option<usize>,
        from: Positions,
    ) -> Positions {
        let mut step = self.step(operand);
        self.seed(&mut step, from, min);
        // 1. exactly `min` repetitions
        let mut at = from;
        let mut done = 0;
        while done < min {
            let next = self.apply(&mut step, at, len);
            if next == 0 {
                return 0;
            }
            if next == at {
                // a fixed point: every further set is this one, up to `min`
                // and beyond, so it is the union of S(min), ..., S(max)
                return at;
            }
            at = next;
            done += 1;
        }
        // 2. up to `max - min` more, expanding only the new positions
        let mut reached = at;
        let mut frontier = at;
        let mut left = max.saturating_sub(min);
        while left > 0 {
            frontier = self.apply(&mut step, frontier, len) & !reached;
            if frontier == 0 {
                break;
            }
            reached |= frontier;
            left -= 1;
        }
        reached
    }

    /// As many repetitions of `operand`, an atom or a character class, as
    /// there are from each position, up to `max`, if that is `min` or more:
    /// what `UnambiguousRepeat` matches.
    fn possessive(
        &self,
        operand: &Operation,
        min: usize,
        max: usize,
        from: Positions,
    ) -> Positions {
        #[cfg(test)]
        if POSSESSIVE_AS_COUNTED.with(Cell::get) {
            return self.counted(operand, min, max, None, from);
        }
        let mut step = self.step(operand);
        self.seed(&mut step, from, min);
        let Step::Mask {
            operand,
            len,
            mut known,
            mut mask,
        } = step
        else {
            unreachable!("turned down by `supports`")
        };
        // `at`: where the repetitions from `from` are after `done` of them
        let mut at = from;
        let mut ends = 0;
        let mut done = 0;
        loop {
            if done == max {
                return ends | at;
            }
            let matched = self.starts(operand, at, &mut known, &mut mask);
            if done >= min {
                // where the operand does not match again, it stops
                ends |= at & !matched;
            }
            at = matched;
            if at == 0 {
                return ends;
            }
            // `matched` holds positions p with p + len <= n < 128
            at <<= len;
            done += 1;
        }
    }

    /// How one repetition of `operand` moves a set of positions: an atom or
    /// a character class gets a mask, filled in as the repetition goes
    /// (`Haystack::starts`).
    fn step<'o>(&self, operand: &'o Operation) -> Step<'o> {
        let len = match operand {
            Operation::Capture(capture) => return self.step(&capture.child_op),
            Operation::Atom(atom) if !atom.atom.is_empty() => atom.atom.len(),
            Operation::CharClass(_) => 1,
            _ => return Step::Ends(operand),
        };
        if len > self.chars.len() {
            // longer than the haystack: it matches nowhere, and nothing is
            // shifted by a length that is not a position
            return Step::Mask {
                operand,
                len: 0,
                known: Positions::MAX,
                mask: 0,
            };
        }
        Step::Mask {
            operand,
            len,
            known: 0,
            mask: 0,
        }
    }

    /// Looks up, in one pass, the positions that `count` repetitions of an
    /// atom or a character class from `from` can start from: from the first
    /// position in `from` to the last plus `count - 1` lengths. The minimum
    /// of a repetition goes through them, so `[0-9]{44}` is one scan of the
    /// haystack instead of 44 lookups of one position; the positions beyond
    /// the minimum are looked up as the repetition reaches them. A count
    /// below 2 needs only `from`, which the first repetition looks up anyway.
    fn seed(&self, step: &mut Step, from: Positions, count: usize) {
        let Step::Mask {
            operand,
            len,
            known,
            mask,
        } = step
        else {
            return;
        };
        let n = self.chars.len();
        if from == 0 || count < 2 || *len == 0 {
            return;
        }
        let first = from.trailing_zeros() as usize;
        let last = (Positions::BITS - 1 - from.leading_zeros()) as usize;
        // a `len` other than 0 is at most n (`step`); past n - len the
        // window is empty
        let last = last
            .saturating_add((count - 1).saturating_mul(*len))
            .min(n - *len);
        let window = below(last + 1) & !below(first);
        self.starts(operand, window, known, mask);
    }

    /// The positions in `set` from which `operand`, an atom or a character
    /// class, matches; those not yet in `known` are looked up and remembered
    /// in `mask`.
    fn starts(
        &self,
        operand: &Operation,
        set: Positions,
        known: &mut Positions,
        mask: &mut Positions,
    ) -> Positions {
        let new = set & !*known;
        if new != 0 {
            *mask |= match operand {
                Operation::Atom(atom) => self.atom_starts(&atom.atom, new),
                Operation::CharClass(class) => self.class_starts(&class.character_class, new),
                _ => unreachable!("`step` makes a mask of an atom or a class only"),
            };
            *known |= new;
        }
        set & *mask
    }

    /// One repetition from the positions in `set`. `len`, when given, is the
    /// length of every match of the operand (checked in debug builds).
    fn apply(&self, step: &mut Step, set: Positions, len: Option<usize>) -> Positions {
        match step {
            // the positions `starts` gives are p with p + len <= n < 128
            Step::Mask {
                operand,
                len,
                known,
                mask,
            } => self.starts(operand, set, known, mask) << *len,
            Step::Ends(operand) => {
                let ends = self.ends(operand, set);
                debug_assert!(
                    len.is_none_or(|len| {
                        let shifted = u32::try_from(len).ok().and_then(|len| set.checked_shl(len));
                        ends & !shifted.unwrap_or(0) == 0
                    }),
                    "a fixed-length operand matched another length"
                );
                ends
            }
        }
    }

    fn atom_ends(&self, atom: &[char], from: Positions) -> Positions {
        let starts = self.atom_starts(atom, from);
        // `starts` holds positions p with p + atom.len() <= n < 128
        if starts == 0 {
            0
        } else {
            starts << atom.len()
        }
    }

    fn class_ends(&self, class: &CharacterClass, from: Positions) -> Positions {
        self.class_starts(class, from) << 1
    }

    /// The positions in `set` from which `atom` matches.
    fn atom_starts(&self, atom: &[char], set: Positions) -> Positions {
        let (n, len) = (self.chars.len(), atom.len());
        if len > n {
            return 0;
        }
        let mut starts = 0;
        for_each(set & below(n - len + 1), |p| {
            count_lookup();
            if self.atom_at(atom, p) {
                starts |= 1 << p;
            }
        });
        starts
    }

    /// The positions in `set` from which `class` matches (the character there
    /// is in it).
    fn class_starts(&self, class: &CharacterClass, set: Positions) -> Positions {
        let mut starts = 0;
        for_each(set & below(self.chars.len()), |p| {
            count_lookup();
            if class.contains(self.chars[p]) {
                starts |= 1 << p;
            }
        });
        starts
    }

    /// Whether `atom` matches the haystack from position `p` (with
    /// `p + atom.len() <= n`), as `Atom` does in the matcher.
    fn atom_at(&self, atom: &[char], p: usize) -> bool {
        let window = &self.chars[p..p + atom.len()];
        if self.case_blind {
            window
                .iter()
                .zip(atom)
                .all(|(&c, &a)| equal_case_blind(c, a))
        } else {
            window == atom
        }
    }
}

#[cfg(test)]
mod tests;
