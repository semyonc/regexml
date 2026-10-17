use std::cell::RefCell;
use std::rc::Rc;

use ahash::{HashMap, HashMapExt, HashSet};

use crate::op_repeat::Repeat;

#[cfg(test)]
use crate::operation::Operation;
use icu_casemap::CaseMapper;

use crate::{
    operation::OperationControl,
    re_compiler::Error,
    re_program::{ReProgram, OPT_HASBACKREFS, OPT_HASBOL},
};

#[derive(Debug)]
pub(crate) struct ReMatcher<'a> {
    // current program
    pub(crate) program: &'a ReProgram,
    // string being matched against
    pub(crate) search: Vec<char>,

    case_mapper: CaseMapper,
    // parenthesized subexpressions
    state: RefCell<State>,
    // whether captures must be correct after backtracking (`Undo`): always,
    // except when only the yes/no answer or the overall match is wanted and
    // the program has no back-references, so that nothing reads them
    track_captures: bool,
    // whether only the yes/no answer is wanted, so that neither the captures
    // nor the order in which matches are found make a difference
    answer_only: bool,
}

#[derive(Debug)]
pub(crate) struct State {
    pub(crate) start_backref: Vec<Option<usize>>,
    pub(crate) end_backref: Vec<Option<usize>>,
    pub(crate) capture_state: CaptureState,
    pub(crate) anchored_match: bool,
    // Undo log of the capture writes made during the current match attempt,
    // most recent last; see `ReMatcher::trail_mark` and `ReMatcher::undo_to`.
    pub(crate) trail: Vec<Undo>,
    // For each group, where in the log its latest logged write is, so that
    // the groups written since a mark can be found without reading the log
    // (`ReMatcher::captures_since`).
    last_write: Vec<Option<usize>>,
    // What a repetition reported from a start position, in order, once it
    // reported all of it; see `ReMatcher::repeat_results`.
    repeat_results: HashMap<(usize, usize), Rc<[RepeatResult]>>,
    repeat_results_size: usize,
    // For each repetition that is not nested in another one, the states and
    // positions it has settled; see `ReMatcher::settled`.
    settled: HashMap<usize, Rc<RefCell<Settled>>>,
}

/// A state of a repetition's search: its position, and what the groups that
/// back-references refer to hold when the program has back-references
/// (`ReMatcher::backref_snapshot`) — the only captures matching reads.
pub(crate) type StateKey = (usize, Option<Box<[Option<usize>]>>);

/// What a repetition that is not nested in another one has settled, for all
/// its iterations in a matcher (`ReMatcher::settled`): states whose every
/// position was reported and rejected by the rest of the expression, and
/// positions reported and rejected. Such a repetition has the same
/// continuation wherever it is matched — what follows it in the expression,
/// up to the end — so what failed once fails again: a search that starts at
/// the next position need not explore it again.
#[derive(Debug, Default)]
pub(crate) struct Settled {
    // the fewest repetitions (at least the minimum) a settled state had
    pub(crate) explored: HashMap<StateKey, usize>,
    // settled states with fewer repetitions than the minimum
    pub(crate) explored_below_min: HashSet<(StateKey, usize)>,
    pub(crate) failed: HashSet<StateKey>,
}

impl State {
    pub(crate) fn new() -> Self {
        Self {
            start_backref: vec![],
            end_backref: vec![],
            capture_state: CaptureState::new(),
            anchored_match: false,
            trail: Vec::new(),
            last_write: Vec::new(),
            repeat_results: HashMap::new(),
            repeat_results_size: 0,
            settled: HashMap::new(),
        }
    }
}

/// A capture write that backtracking may have to take back: the group and
/// the offsets it held before the write.
///
/// Captures are side effects of the matching iterators, so an attempt that
/// is abandoned (an alternative, an iteration of a repeat) would otherwise
/// leave its writes behind, overwriting what the group had captured on the
/// path that is still being followed. Every write is logged here first, and
/// an iterator that gives up undoes the log down to the mark it took when it
/// was created (`CaptureGroupIterator`, `RestoreCaptures`; a fixed-length
/// repetition also keeps a mark per repetition, to back off one at a time),
/// which puts back exactly the state its attempt started from.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Undo {
    /// `capture_state.startn` / `endn` of a group, and where in the log the
    /// group's previous write was (`State::last_write`).
    Paren {
        group_nr: usize,
        start: Option<usize>,
        end: Option<usize>,
        last_write: Option<usize>,
    },
    /// `start_backref` / `end_backref` of a group.
    Backref {
        group_nr: usize,
        start: Option<usize>,
        end: Option<usize>,
    },
}

impl<'a> ReMatcher<'a> {
    pub(crate) fn new(program: &'a ReProgram, search: &str) -> Self {
        let search = search.chars().collect::<Vec<_>>();
        Self {
            program,
            search,
            state: RefCell::new(State::new()),
            case_mapper: CaseMapper::new(),
            track_captures: true,
            answer_only: false,
        }
    }

    /// A matcher for a caller that reads no captured group (only whether
    /// there is a match, and where): without back-references the captures
    /// cannot change the outcome, so they are not kept correct on
    /// backtracking, which costs a log entry per capture.
    pub(crate) fn new_without_captures(program: &'a ReProgram, search: &str) -> Self {
        let mut matcher = Self::new(program, search);
        matcher.track_captures = program.optimization_flags & OPT_HASBACKREFS != 0;
        matcher
    }

    /// A matcher for a caller that only asks whether there is a match
    /// (`is_match`, `is_full_match`): like `new_without_captures`, and
    /// moreover the order in which the matches are found does not matter.
    pub(crate) fn new_for_answer(program: &'a ReProgram, search: &str) -> Self {
        let mut matcher = Self::new_without_captures(program, search);
        matcher.answer_only = !matcher.track_captures;
        matcher
    }

    /// Whether only the yes/no answer is asked, with nothing that could read
    /// a capture (`new_for_answer`).
    pub(crate) fn answer_only(&self) -> bool {
        self.answer_only
    }

    /// Whether captures are kept correct on backtracking (see `Undo`).
    pub(crate) fn tracks_captures(&self) -> bool {
        self.track_captures
    }

    /// Whether a repetition's results may be kept for a second iteration
    /// from the same position (`repeat_results`): without back-references,
    /// what a repetition reports from a position — where it ends, and what
    /// it captures on the way — depends on that position alone.
    pub(crate) fn may_reuse_repeat_results(&self) -> bool {
        #[cfg(test)]
        if NO_REPEAT_RESULTS.with(|no| no.get()) {
            return false;
        }
        self.program.optimization_flags & OPT_HASBACKREFS == 0
    }

    /// Whether the search may try more than one start position: not for a
    /// whole-string match (`is_full_match`) nor for an expression that starts
    /// with `^` outside multi-line mode. Only then is `Settled` of use.
    pub(crate) fn starts_more_than_once(&self) -> bool {
        let starts_with_caret = self.program.optimization_flags & OPT_HASBOL != 0;
        !self.anchored_match() && (!starts_with_caret || self.program.flags.is_multi_line())
    }

    /// What the repetition `repeat`, not nested in another repetition, has
    /// settled in this matcher (`Settled`).
    pub(crate) fn settled(&self, repeat: *const Repeat) -> Rc<RefCell<Settled>> {
        self.state
            .borrow_mut()
            .settled
            .entry(repeat as usize)
            .or_default()
            .clone()
    }

    /// What the repetition `repeat` reported from `position`, if a complete
    /// iteration recorded it (`may_reuse_repeat_results`).
    pub(crate) fn repeat_results(
        &self,
        repeat: *const Repeat,
        position: usize,
    ) -> Option<Rc<[RepeatResult]>> {
        self.state
            .borrow()
            .repeat_results
            .get(&(repeat as usize, position))
            .cloned()
    }

    /// Records the positions of a complete iteration of `repeat` from
    /// `position`. At most `MAX_REPEAT_RESULTS` positions are kept in all, so
    /// that a long input does not make the cache grow without bound.
    pub(crate) fn record_repeat_results(
        &self,
        repeat: *const Repeat,
        position: usize,
        results: Vec<RepeatResult>,
    ) {
        const MAX_REPEAT_RESULTS: usize = 1 << 16;
        let mut state = self.state.borrow_mut();
        if state.repeat_results_size + results.len() > MAX_REPEAT_RESULTS {
            return;
        }
        state.repeat_results_size += results.len();
        state
            .repeat_results
            .insert((repeat as usize, position), results.into());
    }

    pub(crate) fn match_at(&self, i: usize, anchored: bool) -> bool {
        // initialize start pointer, paren cache and paren count
        self.set_paren_count(1);
        self.state.borrow_mut().anchored_match = anchored;
        self.set_paren_start(0, i);

        // allocate backref arrays (unless optimizations indicate otherwise)
        if self.program.optimization_flags & OPT_HASBACKREFS != 0 {
            self.state.borrow_mut().start_backref = vec![None; self.program.max_parens.unwrap()];
            self.state.borrow_mut().end_backref = vec![None; self.program.max_parens.unwrap()];
        }

        // match against string
        let mark = self.trail_mark();
        let mut iter = self.program.operation.matches_iter(self, i);
        if let Some(idx) = iter.next() {
            self.set_paren_end(0, idx);
            // the captures are those of the match: nothing left to undo
            self.clear_trail();
            true
        } else {
            // didn't match; an exhausted iterator has already taken back its
            // captures, so this only makes sure of it
            self.undo_to(mark);
            self.state.borrow_mut().capture_state.paren_count = 0;
            false
        }
    }

    pub(crate) fn matches(&mut self, i: usize) -> bool {
        // clear the captured group state
        self.state.borrow_mut().capture_state = CaptureState::new();
        self.clear_trail();

        // can we optimize the search by looking for new lines?
        if self.program.optimization_flags & OPT_HASBOL == OPT_HASBOL {
            // non multi-line matching with BOL: must match at '0' index
            if !self.program.flags.is_multi_line() {
                return i == 0 && self.check_preconditions(i) && self.match_at(i, false);
            }

            // multi-line matching with BOL: seek to next line
            if self.match_at(i, false) {
                return true;
            }
            // TODO: transliterated as close to the Java code as possible to
            // make sure it works correctly. But can be cleaned up.
            let mut nl: isize = i.try_into().unwrap();
            loop {
                nl = self
                    .search
                    .iter()
                    .enumerate()
                    .skip(nl.try_into().unwrap())
                    .find(|(_, c)| **c == '\n')
                    .map(|(i, _)| i.try_into().unwrap())
                    .unwrap_or(-1)
                    + 1;

                if nl >= self.search.len().try_into().unwrap() || nl <= 0 {
                    // "^" does not match a NL at the end of the string
                    return false;
                } else if self.match_at(nl.try_into().unwrap(), false) {
                    return true;
                }
            }
        }

        // is the string long enough to match?
        let actual_length = self.search.len() - i;
        if actual_length < self.program.minimum_length {
            return false;
        }

        // can we optimize the search by looking for a prefix string?
        if let Some(prefix) = &self.program.prefix {
            // prefixed-anchored matching is possible
            let ignore_case = self.program.flags.is_case_independent();
            for j in i..self.search.len() + 1 - prefix.len() {
                let mut prefix_ok = true;
                if ignore_case {
                    for (k, prefix) in prefix.iter().enumerate() {
                        if !self.equal_case_blind(self.search[k + j], *prefix) {
                            prefix_ok = false;
                            break;
                        }
                    }
                } else {
                    for (k, prefix) in prefix.iter().enumerate() {
                        if self.search[k + j] != *prefix {
                            prefix_ok = false;
                            break;
                        }
                    }
                }

                // see if the whole prefix string matched
                if prefix_ok {
                    // we matched the full prefix at first_char, so try it
                    if self.match_at(j, false) {
                        return true;
                    }
                }
            }
            false
        } else {
            // no prefix known; but the first character must match a predicate
            if let Some(inv_list) = &self.program.initial_char_class {
                for j in i..self.search.len() {
                    if inv_list.contains(self.search[j]) && self.match_at(j, false) {
                        return true;
                    }
                }
                return false;
            }
            // check the preconditions
            if !self.check_preconditions(i) {
                return false;
            }

            // unprefixed matching must try for a match at each character
            for j in i..(self.search.len() + 1) {
                // try a match at index i
                if self.match_at(j, false) {
                    return true;
                }
            }
            false
        }
    }

    pub(crate) fn is_match(&mut self) -> bool {
        self.matches(0)
    }

    /// Whether the program matches the whole search string: a match that
    /// starts at 0 and that the end of the program accepts only at the end of
    /// the string (`anchored_match`, see `EndProgram`), so that every
    /// alternative and repetition count is tried until one reaches it. The
    /// search optimizations of `matches` (prefix, first character class,
    /// preconditions) are not needed for a single start position.
    pub(crate) fn is_full_match(&mut self) -> bool {
        // clear the captured group state
        self.state.borrow_mut().capture_state = CaptureState::new();
        self.clear_trail();

        // what every match needs, as `matches` checks it first: enough input,
        // and the operations the program cannot match without
        // (`ReProgram::preconditions`)
        if self.search.len() < self.program.minimum_length || !self.check_preconditions(0) {
            return false;
        }
        self.match_at(0, true)
    }

    fn check_preconditions(&self, start: usize) -> bool {
        for precondition in &self.program.preconditions {
            if let Some(fixed_position) = precondition.fixed_position {
                let match_ = precondition
                    .operation
                    .matches_iter(self, fixed_position)
                    .next();
                if match_.is_none() {
                    return false;
                }
            } else {
                let mut i = start;
                if i < precondition.min_position {
                    i = precondition.min_position;
                }
                let mut found = false;
                for j in i..self.search.len() {
                    if (precondition.fixed_position.is_none()
                        || precondition.fixed_position == Some(j))
                        && precondition
                            .operation
                            .matches_iter(self, j)
                            .next()
                            .is_some()
                    {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return false;
                }
            }
        }
        true
    }

    pub(crate) fn replace(&mut self, replacement: &[char]) -> Result<Vec<char>, Error> {
        // string to return
        let mut result = Vec::new();

        // start at position 0 and search the whole string
        let mut pos = 0;
        let len = self.search.len();

        let mut first_match = true;
        let mut simple_replacement = false;

        // try a match at each position
        while pos < len && self.matches(pos) {
            // append chars from input string before match
            // TODO: what happens if this returns None as there is no paren start?
            if let Some(start) = self.get_paren_start(0) {
                result.extend(&self.search[pos..start]);
            }
            if first_match {
                simple_replacement = self.program.flags.is_literal();
                first_match = false;
            }

            if !simple_replacement {
                // process references to captured substrings
                let max_capture = self.program.max_parens.unwrap() - 1;
                simple_replacement = true;
                let mut i = 0;
                while i < replacement.len() {
                    let ch = replacement[i];
                    match ch {
                        '\\' => {
                            simple_replacement = false;
                            i += 1;
                            let index = i;
                            if index >= replacement.len() {
                                return Err(Error::InvalidReplacementString(
                                    "Invalid escape at end of replacement string".to_string(),
                                ));
                            }
                            let ch = replacement[index];
                            match ch {
                                '\\' | '$' => {
                                    result.push(ch);
                                }
                                _ => {
                                    return Err(Error::InvalidReplacementString(
                                        format!("Invalid escape '{}' in replacement string", ch)
                                            .to_string(),
                                    ))
                                }
                            }
                        }
                        '$' => {
                            simple_replacement = false;
                            i += 1;
                            let index = i;
                            if index >= replacement.len() {
                                return Err(Error::InvalidReplacementString(
                                    "Invalid escape at end of replacement string".to_string(),
                                ));
                            }
                            let ch = replacement[index];
                            if !ch.is_ascii_digit() {
                                return Err(Error::InvalidReplacementString(
                                    "$ in replacement string must be followed by a digit"
                                        .to_string(),
                                ));
                            }
                            let mut n = (ch as usize) - ('0' as usize);
                            if max_capture <= 9 {
                                if max_capture >= n {
                                    let captured = self.get_paren(n);
                                    if let Some(captured) = captured {
                                        result.extend(captured);
                                    }
                                }
                            } else {
                                loop {
                                    i += 1;
                                    if i >= replacement.len() {
                                        break;
                                    }
                                    let ch = replacement[i];
                                    if ch.is_ascii_digit() {
                                        let m = n * 10 + ((ch as usize) - ('0' as usize));
                                        if m > max_capture {
                                            i -= 1;
                                            break;
                                        } else {
                                            n = m;
                                        }
                                    } else {
                                        i -= 1;
                                        break;
                                    }
                                }
                                let captured = self.get_paren(n);
                                if let Some(captured) = captured {
                                    result.extend(captured);
                                }
                            }
                        }
                        _ => {
                            result.push(ch);
                        }
                    }
                    i += 1;
                }
            } else {
                // append substitution without processing backreferences
                result.extend(replacement);
            }

            // move forward, skipping past match
            let mut newpos = self.get_paren_end(0).unwrap();

            if newpos == pos {
                newpos += 1;
            }

            // try new position
            pos = newpos;
        }

        // if no matches were found, return the input unchanged
        if first_match {
            return Ok(self.search.to_vec());
        }

        // if there's remaining input, append it
        result.extend(self.search[pos..len].iter());

        // return the string buffer
        Ok(result)
    }

    pub(crate) fn is_new_line(&self, i: usize) -> bool {
        self.search[i] == '\n'
    }

    #[cfg(test)]
    pub(crate) fn operation_matches(&self, op: Operation) -> Vec<String> {
        let start_position = 0;
        let positions = op.matches_iter(self, start_position);
        let ranges = positions.map(|e| start_position..e);
        ranges
            .map(|r| self.search[r].iter().collect::<String>())
            .collect()
    }

    pub(crate) fn equal_case_blind(&self, a: char, b: char) -> bool {
        if a == b {
            return true;
        }
        let lowercase_a = self.case_mapper.simple_lowercase(a);
        let lowercase_b = self.case_mapper.simple_lowercase(b);
        if lowercase_a == lowercase_b {
            return true;
        }
        false
    }

    // state related

    /// Of `groups`, those written since `mark` (on the path being followed:
    /// the writes of abandoned attempts have been taken back), with what they
    /// hold now.
    pub(crate) fn captures_since(&self, mark: usize, groups: &[usize]) -> Box<[Capture]> {
        let state = self.state.borrow();
        let captures = &state.capture_state;
        groups
            .iter()
            .filter(|&&group_nr| {
                state
                    .last_write
                    .get(group_nr)
                    .copied()
                    .flatten()
                    .is_some_and(|at| at >= mark)
            })
            .map(|&group_nr| Capture {
                group_nr,
                start: captures.startn.get(group_nr).copied().flatten(),
                end: captures.endn.get(group_nr).copied().flatten(),
            })
            .collect()
    }

    /// Empties the capture log, at the start of a search or after a match.
    fn clear_trail(&self) {
        let state = &mut *self.state.borrow_mut();
        state.trail.clear();
        state.last_write.clear();
    }

    /// Records that group `group_nr` matched the empty string at `position`,
    /// as `Capture` does when its operand matches there.
    pub(crate) fn capture_empty(&self, group_nr: usize, position: usize) {
        if group_nr >= self.paren_count() {
            self.set_paren_count(group_nr + 1);
        }
        self.capture(group_nr, position, position);
    }

    /// Writes a group as `captures_since` reported it, logged like `capture`.
    pub(crate) fn restore_capture(&self, capture: &Capture) {
        match (capture.start, capture.end) {
            (Some(start), Some(end)) => {
                if capture.group_nr >= self.paren_count() {
                    self.set_paren_count(capture.group_nr + 1);
                }
                self.capture(capture.group_nr, start, end);
            }
            _ => self.unbind(capture.group_nr),
        }
    }

    /// What the groups that back-references refer to hold, as one slice:
    /// everything matching reads of the captures.
    pub(crate) fn backref_snapshot(&self) -> Box<[Option<usize>]> {
        self.snapshot_of(&self.program.back_referenced_groups)
    }

    /// What `groups` hold for back-references, as one slice.
    pub(crate) fn snapshot_of(&self, groups: &[usize]) -> Box<[Option<usize>]> {
        let state = self.state.borrow();
        groups
            .iter()
            .flat_map(|&group_nr| {
                // the arrays are made by `match_at`; the preconditions run
                // before it
                [
                    state.start_backref.get(group_nr).copied().flatten(),
                    state.end_backref.get(group_nr).copied().flatten(),
                ]
            })
            .collect()
    }

    pub(crate) fn start_backref(&self, i: usize) -> Option<usize> {
        self.state.borrow().start_backref[i]
    }

    pub(crate) fn end_backref(&self, i: usize) -> Option<usize> {
        self.state.borrow().end_backref[i]
    }

    pub(crate) fn anchored_match(&self) -> bool {
        self.state.borrow().anchored_match
    }

    // capture state related

    pub(crate) fn get_paren(&self, group_nr: usize) -> Option<&[char]> {
        if group_nr < self.paren_count() {
            if let (Some(start), Some(end)) =
                (self.get_paren_start(group_nr), self.get_paren_end(group_nr))
            {
                return Some(&self.search[start..end]);
            }
        }
        None
    }

    pub(crate) fn get_paren_start(&self, group_nr: usize) -> Option<usize> {
        if group_nr < self.state.borrow().capture_state.startn.len() {
            return self.state.borrow().capture_state.startn[group_nr];
        }
        None
    }

    /// Sets the start of the paren level
    /// which is the paren level, and i is index in input.
    pub(crate) fn set_paren_start(&self, group_nr: usize, position: usize) {
        self.state
            .borrow_mut()
            .capture_state
            .set_paren_start(group_nr, position)
    }

    pub(crate) fn get_paren_end(&self, group_nr: usize) -> Option<usize> {
        if group_nr < self.state.borrow().capture_state.endn.len() {
            return self.state.borrow().capture_state.endn[group_nr];
        }
        None
    }

    pub(crate) fn set_paren_end(&self, group_nr: usize, position: usize) {
        self.state
            .borrow_mut()
            .capture_state
            .set_paren_end(group_nr, position)
    }

    pub(crate) fn paren_count(&self) -> usize {
        self.state.borrow().capture_state.paren_count
    }

    pub(crate) fn set_paren_count(&self, count: usize) {
        self.state.borrow_mut().capture_state.paren_count = count;
    }

    /// Records that group `group_nr` has matched `start..end`, logging what
    /// it held before (`Undo`) so that backtracking can put that back.
    pub(crate) fn capture(&self, group_nr: usize, start: usize, end: usize) {
        let state = &mut *self.state.borrow_mut();
        let captures = &mut state.capture_state;
        if self.track_captures {
            let last_write = log_write(&mut state.last_write, group_nr, state.trail.len());
            state.trail.push(Undo::Paren {
                group_nr,
                start: captures.startn.get(group_nr).copied().flatten(),
                end: captures.endn.get(group_nr).copied().flatten(),
                last_write,
            });
        }
        captures.set_paren_start(group_nr, start);
        captures.set_paren_end(group_nr, end);
        if self.program.optimization_flags & OPT_HASBACKREFS != 0 {
            state.trail.push(Undo::Backref {
                group_nr,
                start: state.start_backref[group_nr],
                end: state.end_backref[group_nr],
            });
            state.start_backref[group_nr] = Some(start);
            state.end_backref[group_nr] = Some(end);
        }
    }

    /// Records the start of an attempt to match group `group_nr` for its
    /// back-references (only when the program has back-references), logged
    /// like `capture`.
    pub(crate) fn open_backref(&self, group_nr: usize, start: usize) {
        let state = &mut *self.state.borrow_mut();
        state.trail.push(Undo::Backref {
            group_nr,
            start: state.start_backref[group_nr],
            end: state.end_backref[group_nr],
        });
        state.start_backref[group_nr] = Some(start);
    }

    /// Sets group `group_nr` back to "not matched", logged like `capture`.
    pub(crate) fn unbind(&self, group_nr: usize) {
        let state = &mut *self.state.borrow_mut();
        let captures = &mut state.capture_state;
        if group_nr < captures.startn.len() && group_nr < captures.endn.len() {
            let (start, end) = (captures.startn[group_nr], captures.endn[group_nr]);
            if start.is_some() || end.is_some() {
                let last_write = log_write(&mut state.last_write, group_nr, state.trail.len());
                state.trail.push(Undo::Paren {
                    group_nr,
                    start,
                    end,
                    last_write,
                });
                captures.startn[group_nr] = None;
                captures.endn[group_nr] = None;
            }
        }
        if self.program.optimization_flags & OPT_HASBACKREFS != 0 {
            let (start, end) = (state.start_backref[group_nr], state.end_backref[group_nr]);
            if start.is_some() || end.is_some() {
                state.trail.push(Undo::Backref {
                    group_nr,
                    start,
                    end,
                });
                state.start_backref[group_nr] = None;
                state.end_backref[group_nr] = None;
            }
        }
    }

    /// The current length of the capture log: an iterator takes it when it is
    /// created and passes it to `undo_to` when it gives up.
    pub(crate) fn trail_mark(&self) -> usize {
        self.state.borrow().trail.len()
    }

    /// Takes back every capture write logged after `mark`, most recent first,
    /// restoring the offsets the groups held when the mark was taken.
    pub(crate) fn undo_to(&self, mark: usize) {
        let state = &mut *self.state.borrow_mut();
        while state.trail.len() > mark {
            match state.trail.pop().unwrap() {
                Undo::Paren {
                    group_nr,
                    start,
                    end,
                    last_write,
                } => {
                    state.capture_state.startn[group_nr] = start;
                    state.capture_state.endn[group_nr] = end;
                    state.last_write[group_nr] = last_write;
                }
                Undo::Backref {
                    group_nr,
                    start,
                    end,
                } => {
                    state.start_backref[group_nr] = start;
                    state.end_backref[group_nr] = end;
                }
            }
        }
    }
}

// Records that the write of group `group_nr` about to be logged at `at` is
// its latest, and returns where the previous one was.
fn log_write(last_write: &mut Vec<Option<usize>>, group_nr: usize, at: usize) -> Option<usize> {
    if group_nr >= last_write.len() {
        last_write.resize(group_nr + 1, None);
    }
    last_write[group_nr].replace(at)
}

#[cfg(test)]
thread_local! {
    // Turns `ReMatcher::repeat_results` off, so that tests can compare.
    pub(crate) static NO_REPEAT_RESULTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    // Turns `Settled` and the shortcuts of `Repeat::later_successors` off.
    pub(crate) static NO_SHORTCUTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A position a repetition reported, with the groups it had written on the
/// way to it (`ReMatcher::captures_since`) when captures are kept, and
/// whether it was the zero-repetitions result (which leaves the operand's
/// groups unmatched).
#[derive(Debug)]
pub(crate) struct RepeatResult {
    pub(crate) position: usize,
    pub(crate) zero: bool,
    pub(crate) captures: Box<[Capture]>,
}

/// A group and what it holds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Capture {
    pub(crate) group_nr: usize,
    pub(crate) start: Option<usize>,
    pub(crate) end: Option<usize>,
}

#[derive(Debug)]
pub(crate) struct CaptureState {
    // Number of subexpressions matched (num open parens + 1)
    pub(crate) paren_count: usize,
    // Lazily-allocated array of sub-expression starts
    pub(crate) startn: Vec<Option<usize>>,
    // Lazily-allocated array of sub-expression ends
    pub(crate) endn: Vec<Option<usize>>,
}

impl CaptureState {
    fn new() -> Self {
        Self {
            paren_count: 0,
            startn: vec![None, None, None],
            endn: vec![None, None, None],
        }
    }

    pub(crate) fn set_paren_start(&mut self, group_nr: usize, position: usize) {
        // if we use a group nr that' hns bigger than the len
        // In the Java version this has complicated array doubling and copying
        // code, but it appears it can be just extended with None values.
        while group_nr >= self.startn.len() {
            self.startn.extend(vec![None; self.startn.len()]);
        }
        self.startn[group_nr] = Some(position);
    }

    pub(crate) fn set_paren_end(&mut self, group_nr: usize, position: usize) {
        // see set_paren_start
        while group_nr > self.endn.len() - 1 {
            self.endn.extend(vec![None; self.endn.len()]);
        }
        self.endn[group_nr] = Some(position);
    }
}
