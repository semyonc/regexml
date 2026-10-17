use std::cell::RefCell;
use std::rc::Rc;

use ahash::{HashMap, HashMapExt, HashSet, HashSetExt};

use crate::{
    character_class::CharacterClass,
    operation::{
        captures, groups_of, unbind_groups, Operation, OperationControl, RepeatOperation,
        RestoreCaptures, MATCHES_ZLS_ANYWHERE, MATCHES_ZLS_NEVER,
    },
    re_flags::ReFlags,
    re_matcher::{ReMatcher, RepeatResult, Settled, StateKey},
    re_program::OPT_HASBACKREFS,
};

// Handle a repetition (with possible min and max) where the
// size of the repeated unit is variable.
#[derive(Debug, Clone)]
pub(crate) struct Repeat {
    pub(crate) operation: Box<Operation>,
    pub(crate) min: usize,
    pub(crate) max: usize,
    pub(crate) greedy: bool,
    // whether it is inside another repetition (`OperationControl::mark_nested`)
    pub(crate) nested: bool,
    // what back-references can make of its states (`Repeat::analysis`)
    analysis: std::sync::OnceLock<Analysis>,
}

// What the captures of a state can change, when the program has
// back-references: whether the operand reads captures itself (it contains a
// back-reference), and else the back-referenced groups inside the operand
// that a repetition does not always write — the only captures a state passes
// on to the states after it. (A group a repetition always writes is
// overwritten; one outside the operand is the same for every state.)
#[derive(Debug, Clone)]
struct Analysis {
    operand_reads: bool,
    carried: Vec<usize>,
}

// Whether an operation contains a back-reference.
fn has_back_reference(operation: &Operation) -> bool {
    matches!(operation, Operation::BackReference(_))
        || operation.children().iter().any(has_back_reference)
}

// The groups every match of an operation writes (some of them: what is
// certain from the structure).
fn always_written(operation: &Operation, groups: &mut Vec<usize>) {
    match operation {
        Operation::Capture(capture) => {
            groups.push(capture.group_nr);
            always_written(&capture.child_op, groups);
        }
        Operation::Sequence(_) => {
            for child in operation.children() {
                always_written(&child, groups);
            }
        }
        _ => {
            if let Some(repeat) = operation.repeat_operation() {
                if repeat.min() > 0 {
                    always_written(&repeat.child(), groups);
                }
            }
        }
    }
}

impl Repeat {
    pub(crate) fn new(operation: Operation, min: usize, max: usize, greedy: bool) -> Self {
        Self {
            operation: Box::new(operation),
            min,
            max,
            greedy,
            nested: true,
            analysis: std::sync::OnceLock::new(),
        }
    }
}

impl OperationControl for Repeat {
    fn get_match_length(&self) -> Option<usize> {
        self.operation.get_match_length().and_then(|match_length| {
            if self.min == self.max {
                Some(self.min * match_length)
            } else {
                None
            }
        })
    }

    fn get_minimum_match_length(&self) -> usize {
        self.min * self.operation.get_minimum_match_length()
    }

    fn get_initial_character_class(&self, case_blind: bool) -> CharacterClass {
        self.operation.get_initial_character_class(case_blind)
    }

    fn optimize(self, flags: &ReFlags) -> Operation {
        let operation = self.operation.optimize(flags);
        let min = if self.min == 0 && operation.matches_empty_string() == MATCHES_ZLS_ANYWHERE {
            // turns (a?)* into (a?)+
            1
        } else {
            self.min
        };
        Operation::from(Repeat {
            operation: Box::new(operation),
            min,
            max: self.max,
            greedy: self.greedy,
            nested: self.nested,
            analysis: std::sync::OnceLock::new(),
        })
    }

    fn matches_empty_string(&self) -> u32 {
        if self.min == 0 {
            MATCHES_ZLS_ANYWHERE
        } else {
            self.operation.matches_empty_string()
        }
    }

    fn contains_capturing_expressions(&self) -> bool {
        matches!(self.operation.as_ref(), Operation::Capture(_))
            || self.operation.contains_capturing_expressions()
    }

    fn matches_iter<'a>(
        &'a self,
        matcher: &'a ReMatcher<'a>,
        position: usize,
    ) -> Box<dyn Iterator<Item = usize> + 'a> {
        if matcher.tracks_captures() && captures(&self.operation) {
            let mark = matcher.trail_mark();
            RestoreCaptures::wrap(matcher, mark, self.repeat_iter(matcher, position))
        } else {
            self.repeat_iter(matcher, position)
        }
    }

    fn children(&self) -> Vec<Operation> {
        vec![self.operation.as_ref().clone()]
    }

    fn collect_groups(&self, groups: &mut Vec<usize>) {
        self.operation.collect_groups(groups);
    }

    fn mark_nested(&mut self, within_repetition: bool) {
        self.nested = within_repetition;
        self.operation.mark_nested(true);
    }
}

impl Repeat {
    fn repeat_iter<'a>(
        &'a self,
        matcher: &'a ReMatcher<'a>,
        position: usize,
    ) -> Box<dyn Iterator<Item = usize> + 'a> {
        // with zero occurrences allowed, the groups of a capturing operand
        // are left unmatched when zero occurrences are reported
        let zero_groups = (self.min == 0 && matcher.tracks_captures() && captures(&self.operation))
            .then(|| groups_of(&self.operation));
        // Where a repetition can go from a state, and whether the rest of
        // the expression matches from where it ends, depend on the state's
        // position and, through back-references, on what the groups have
        // captured — nothing else — so a state that has been explored once
        // need not be explored again.
        let backrefs = matcher.program.optimization_flags & OPT_HASBACKREFS != 0;
        if matcher.may_reuse_repeat_results() {
            if let Some(results) = matcher.repeat_results(self, position) {
                return Box::new(Replay {
                    matcher,
                    results,
                    next: 0,
                    mark: matcher.trail_mark(),
                    zero_groups,
                });
            }
        }
        Box::new(RepeatIterator {
            repeat: self,
            // a repetition that is not nested in another one is matched again
            // from a position only where it failed before (`Settled`)
            record: (matcher.may_reuse_repeat_results() && self.nested).then(Vec::new),
            record_groups: if matcher.tracks_captures() && self.nested {
                groups_of(&self.operation)
            } else {
                Vec::new()
            },
            start: position,
            mark: matcher.trail_mark(),
            matcher,
            operation: self.operation.as_ref(),
            min: self.effective_min(matcher),
            max: self.max,
            greedy: self.greedy,
            states: vec![State {
                position,
                count: 0,
                operand: None,
                reported: false,
                last: false,
                undo: None,
            }],
            zero_groups,
            unbound: None,
            backrefs,
            entered: HashMap::new(),
            entered_below_min: HashSet::new(),
            reported: HashSet::new(),
            // a repetition that is not nested is iterated once per start
            // position at most, so tables as long as the rest of the input
            // cost no more than the search
            dense: (!backrefs && !self.nested).then(|| Dense {
                entered: vec![u32::MAX; matcher.search.len() + 1 - position],
                reported: vec![false; matcher.search.len() + 1 - position],
            }),
            settled: (!self.nested && !no_shortcuts() && matcher.starts_more_than_once())
                .then(|| matcher.settled(self)),
            last_report: None,
            // When only the answer counts, an operand that is closed under
            // repetition (`closed`) reaches from any of its results only
            // positions it reached already: the states after the first
            // repetition need no successors.
            later: self.later_successors(matcher, self.effective_min(matcher)),
        })
    }
}

impl Repeat {
    // The successors of the states with at least one repetition, in this
    // matcher. When the operand is closed under repetition (`closed`) and the
    // minimum is at most one, a state after the first repetition can only
    // reach positions the first repetition reached already. Where neither the
    // order of the matches nor the captures matter, that state needs no
    // successors. Where they do, it needs none either when the operand is
    // `greedy_closed`, because then every position it could reach was
    // reported before it — except, for a greedy repetition whose captures are
    // kept, the empty repetition beyond the minimum that ends the search
    // there, which comes first and carries its own captures. All this holds
    // only when captures cannot change what matches (no back-references).
    fn later_successors(&self, matcher: &ReMatcher, min: usize) -> Later {
        if no_shortcuts() {
            return Later::All;
        }
        if min > 1 || matcher.program.optimization_flags & OPT_HASBACKREFS != 0 {
            return Later::All;
        }
        if matcher.answer_only() && closed(&self.operation) {
            return Later::None;
        }
        if !greedy_closed(&self.operation) {
            return Later::All;
        }
        if !self.greedy || !matcher.tracks_captures() {
            return Later::None;
        }
        let mut writes = Vec::new();
        if empty_writes(&self.operation, &mut writes) {
            Later::EmptyOnly(writes)
        } else if self.operation.matches_empty_string() == MATCHES_ZLS_NEVER {
            Later::None
        } else {
            Later::All
        }
    }
}

impl Repeat {
    fn analysis(&self, program_groups: &[usize]) -> &Analysis {
        self.analysis.get_or_init(|| {
            let operand_reads = has_back_reference(&self.operation);
            let inside = groups_of(&self.operation);
            let mut written = Vec::new();
            always_written(&self.operation, &mut written);
            let carried = program_groups
                .iter()
                .copied()
                .filter(|group| inside.contains(group) && !written.contains(group))
                .collect();
            Analysis {
                operand_reads,
                carried,
            }
        })
    }

    // The minimum the search uses. An operand closed under repetition that
    // can match the empty string anywhere matches, repeated any number of
    // times from one on, what it matches once (O O ⊆ O, and O ⊆ O O through
    // its empty match), so a minimum of two or more is a minimum of one —
    // where only the answer counts, or for a greedy repetition of a
    // `greedy_closed` operand, whose further repetitions up to the minimum
    // all match the empty string there, last, with the captures of an empty
    // repetition beyond the minimum (`Later::EmptyOnly`).
    fn effective_min(&self, matcher: &ReMatcher) -> usize {
        if self.min > 1
            && !no_shortcuts()
            && matcher.program.optimization_flags & OPT_HASBACKREFS == 0
            && self.operation.matches_empty_string() == MATCHES_ZLS_ANYWHERE
            && ((matcher.answer_only() && closed(&self.operation))
                || (self.greedy && greedy_closed(&self.operation)))
        {
            1
        } else {
            self.min
        }
    }
}

impl RepeatOperation for Repeat {
    fn child(&self) -> Operation {
        self.operation.as_ref().clone()
    }

    fn min(&self) -> usize {
        self.min
    }

    fn max(&self) -> usize {
        self.max
    }

    fn greedy(&self) -> bool {
        self.greedy
    }
}

// A state of the search a repetition makes: `count` repetitions of the
// operand done, ending at `position`. Its successors are the states one
// repetition further, one per match of the operand starting at `position`,
// in the order the operand reports them.
struct State<'a> {
    position: usize,
    count: usize,
    // the operand's matches from `position`, for the successors; `None`
    // before the first one is asked for and once they are exhausted
    operand: Option<Box<dyn Iterator<Item = usize> + 'a>>,
    // whether this state's own position has been reported (or skipped)
    reported: bool,
    // whether it has no successors: reached with an empty repetition beyond
    // the minimum, after which there is no further repetition, or one whose
    // successors would all have been reached already (`Later`)
    last: bool,
    // for a state made up without the operand (`Later::EmptyOnly`): the
    // capture log's length before its captures, to take them back with it
    undo: Option<usize>,
}

// The positions a repetition can end at, in the order of a backtracking
// search: a depth-first walk of the states, which reports a state's position
// after its successors when greedy ("the longest possible substring", F&O 3.1
// §5.6.1.2) and before them when reluctant ("the shortest possible substring
// consistent with the match as a whole succeeding"), and reports only states
// with at least `min` repetitions.
//
// Up to the minimum, a repetition may match the empty string like any other,
// so `(a|){2}` matches "". Beyond it, a repetition that matches the empty
// string is the last one: it ends where it starts, so going on from there
// would only reach states already reached, and would let a repetition of an
// operand that can match the empty string go on for ever. It is still taken,
// as in the Java original and in Perl's and Python's engines, so that `(a|)*`
// over "aa" ends with an empty repetition and captures "" (ECMAScript's
// RepeatMatcher does not take it).
//
// This replaces the Java original's iterators, which kept at most one way of
// matching each repetition when reluctant, explored the repetition twice when
// zero occurrences were allowed, and guarded against endless zero-length
// matches with a counter (`ForceProgressIterator`) that also cut off
// legitimate results — `^([ab]*)+b$` did not match "ab".
struct RepeatIterator<'a> {
    // the repetition, and the positions reported so far when they are to be
    // kept (`ReMatcher::repeat_results`), and where it started
    repeat: &'a Repeat,
    record: Option<Vec<RepeatResult>>,
    // the operand's groups, whose captures a recorded result keeps
    record_groups: Vec<usize>,
    start: usize,
    // the capture log's length when it started
    mark: usize,
    matcher: &'a ReMatcher<'a>,
    operation: &'a Operation,
    min: usize,
    max: usize,
    greedy: bool,
    // the path from the starting state (zero repetitions) to the current one
    states: Vec<State<'a>>,
    // the groups of a capturing operand when zero occurrences are allowed,
    // and the capture log's length before they were left unmatched for
    // reporting zero occurrences
    zero_groups: Option<Vec<usize>>,
    unbound: Option<usize>,
    // whether the program has back-references, so that a state is known by
    // its position together with what the groups hold (`key`)
    backrefs: bool,
    // for each state key, the fewest repetitions with which a state with that
    // key (and at least `min` repetitions) has been entered: a state with the
    // same key and as many or more repetitions can only reach what that one
    // reached
    entered: HashMap<Key, usize>,
    // the states entered with fewer than `min` repetitions
    entered_below_min: HashSet<(Key, usize)>,
    // the state keys already reported: the rest of the expression failed
    // from them, and would fail again
    reported: HashSet<Key>,
    // `entered` (from the minimum on) and `reported` as tables indexed by
    // the position from the start, when a state is known by its position
    dense: Option<Dense>,
    // what this repetition settled in earlier iterations, when it is not
    // nested in another repetition (`Settled`), and the key of the position
    // it reported last, which is settled when it is asked for the next one
    settled: Option<Rc<RefCell<Settled>>>,
    last_report: Option<Key>,
    // the successors of the states after the first repetition
    // (`Repeat::later_successors`)
    later: Later,
}

#[cfg(test)]
thread_local! {
    // The operand matches the repetitions have asked for: the work of the
    // search, counted for the tests that bound it.
    pub(crate) static STEPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

// Whether a test has turned the shortcuts off (`NO_SHORTCUTS`).
fn no_shortcuts() -> bool {
    #[cfg(test)]
    {
        crate::re_matcher::NO_SHORTCUTS.with(|no| no.get())
    }
    #[cfg(not(test))]
    {
        false
    }
}

// Which successors the states with at least one repetition have.
enum Later {
    // every match of the operand
    All,
    // none: the first repetition reached everything already
    None,
    // only an empty repetition beyond the minimum, the last one, which the
    // operand makes with these captures (`empty_writes`)
    EmptyOnly(Vec<EmptyWrite>),
}

// A capture the operand makes when it matches the empty string at a position
// as its last match: a group set to the empty string there, or the groups
// left unmatched (the zero repetitions of a repetition inside).
#[derive(Debug, Clone)]
enum EmptyWrite {
    Set(usize),
    Unbind(Vec<usize>),
}

// Whether an operand is closed under repetition (`closed`) and reports its
// matches as a greedy repetition does: each position after every position it
// can reach from there, so that when it reports a position, everything a
// further repetition from that position could reach is already reported.
fn greedy_closed(operation: &Operation) -> bool {
    match operation {
        Operation::Capture(capture) => greedy_closed(&capture.child_op),
        Operation::Repeat(repeat) => repeat.greedy && repeat.max == usize::MAX,
        Operation::GreedyFixed(_) => operation
            .repeat_operation()
            .is_some_and(|repeat| repeat.max() == usize::MAX),
        _ => false,
    }
}

// The captures a `greedy_closed` operand makes with its empty match, the
// last match it reports, at any position; `false` when it cannot match the
// empty string everywhere (or when that is not known).
fn empty_writes(operation: &Operation, writes: &mut Vec<EmptyWrite>) -> bool {
    match operation {
        Operation::Capture(capture) => {
            empty_writes(&capture.child_op, writes) && {
                writes.push(EmptyWrite::Set(capture.group_nr));
                true
            }
        }
        // zero repetitions, after every repetition that matches something;
        // they leave the operand's groups unmatched
        Operation::Repeat(repeat)
            if repeat.min == 0 && repeat.operation.matches_empty_string() == MATCHES_ZLS_NEVER =>
        {
            writes.push(EmptyWrite::Unbind(groups_of(&repeat.operation)));
            true
        }
        // its first repetitions match the empty string
        Operation::Repeat(repeat) if repeat.min > 0 => empty_writes(&repeat.operation, writes),
        Operation::GreedyFixed(_) => {
            let repeat = operation.repeat_operation().unwrap();
            repeat.min() == 0 && {
                writes.push(EmptyWrite::Unbind(groups_of(&repeat.child())));
                true
            }
        }
        _ => false,
    }
}

type Key = StateKey;

struct Dense {
    // the fewest repetitions (its class) a state entered had, or `u32::MAX`
    entered: Vec<u32>,
    reported: Vec<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    Full,
    ReportOnly,
    Skip,
}

// Whether an operand's matches are closed under repetition: whatever it
// matches from a position it matched from, it matched from the first one too.
// So is a repetition with no upper bound (X{n,} twice is X{2n,}, part of
// X{n,}), also in a group.
fn closed(operation: &Operation) -> bool {
    match operation {
        Operation::Capture(capture) => closed(&capture.child_op),
        _ => operation
            .repeat_operation()
            .is_some_and(|repeat| repeat.max() == usize::MAX),
    }
}

impl RepeatIterator<'_> {
    fn key(&self, position: usize) -> Key {
        (
            position,
            self.backrefs.then(|| self.matcher.backref_snapshot()),
        )
    }

    // The key that decides which states a state reaches: its position, and
    // the captures it passes on (`Analysis`).
    fn successor_key(&self, position: usize) -> Key {
        if !self.backrefs {
            return (position, None);
        }
        let analysis = self
            .repeat
            .analysis(&self.matcher.program.back_referenced_groups);
        if analysis.operand_reads {
            self.key(position)
        } else if analysis.carried.is_empty() {
            (position, None)
        } else {
            (position, Some(self.matcher.snapshot_of(&analysis.carried)))
        }
    }

    // The number of repetitions that tells states at the same position
    // apart: with no upper bound, every count from the minimum on leaves the
    // same repetitions to do.
    fn class(&self, count: usize) -> usize {
        if count >= self.min && self.max == usize::MAX {
            self.min
        } else {
            count
        }
    }

    // Whether the state `count` repetitions at `position` is worth entering,
    // and if so, whether for its successors too or only for its own
    // position: with back-references, a state whose successors another state
    // reached may still differ from it in what its own position meets in the
    // rest of the expression.
    fn enter(&mut self, position: usize, count: usize) -> Entry {
        if self.explore(position, count) {
            Entry::Full
        } else if self.backrefs && !self.reported.contains(&self.key(position)) {
            Entry::ReportOnly
        } else {
            Entry::Skip
        }
    }

    // Whether the successors of the state `count` repetitions at `position`
    // are still to be explored (and records that they now are).
    fn explore(&mut self, position: usize, count: usize) -> bool {
        let count = self.class(count);
        let key = self.successor_key(position);
        if let Some(settled) = &self.settled {
            let settled = settled.borrow();
            let done = if count < self.min {
                settled.explored_below_min.contains(&(key.clone(), count))
            } else {
                settled
                    .explored
                    .get(&key)
                    .is_some_and(|&fewest| fewest <= count)
            };
            if done {
                return false;
            }
        }
        if count < self.min {
            // below the minimum, the number of repetitions still to do
            // matters too
            return self.entered_below_min.insert((key, count));
        }
        if let Some(dense) = &mut self.dense {
            let entered = &mut dense.entered[position - self.start];
            if *entered as usize <= count {
                return false;
            }
            *entered = count.min(u32::MAX as usize - 1) as u32;
            return true;
        }
        match self.entered.get(&key) {
            Some(&fewest) if fewest <= count => false,
            _ => {
                self.entered.insert(key, count);
                true
            }
        }
    }

    // The current state is done: every position it could reach was reported
    // and rejected.
    fn settle(&mut self) {
        let Some(settled) = &self.settled else {
            return;
        };
        let state = self.states.last().unwrap();
        if state.last {
            return;
        }
        let (position, count) = (state.position, self.class(state.count));
        let key = self.successor_key(position);
        let mut settled = settled.borrow_mut();
        if count < self.min {
            settled.explored_below_min.insert((key, count));
        } else {
            let fewest = settled.explored.entry(key).or_insert(count);
            *fewest = (*fewest).min(count);
        }
    }

    // Whether the current state's position should be reported now.
    fn report(&mut self) -> Option<usize> {
        let state = self.states.last_mut().unwrap();
        state.reported = true;
        if state.count < self.min {
            return None;
        }
        let position = state.position;
        let count = state.count;
        let key = self.key(position);
        if self
            .settled
            .as_ref()
            .is_some_and(|settled| settled.borrow().failed.contains(&key))
        {
            return None;
        }
        if let Some(dense) = &mut self.dense {
            if std::mem::replace(&mut dense.reported[position - self.start], true) {
                return None;
            }
        } else if !self.reported.insert(key.clone()) {
            return None;
        }
        if self.settled.is_some() {
            self.last_report = Some(key);
        }
        if count == 0 {
            if let Some(groups) = &self.zero_groups {
                self.unbound = Some(unbind_groups(self.matcher, groups));
            }
        }
        if let Some(record) = &mut self.record {
            let captures = if self.matcher.tracks_captures() {
                self.matcher.captures_since(self.mark, &self.record_groups)
            } else {
                Box::default()
            };
            record.push(RepeatResult {
                position,
                zero: count == 0,
                captures,
            });
        }
        Some(position)
    }
}

impl Iterator for RepeatIterator<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(mark) = self.unbound.take() {
            self.matcher.undo_to(mark);
        }
        if let Some(key) = self.last_report.take() {
            // asked again: the rest of the expression failed from there
            self.settled
                .as_ref()
                .unwrap()
                .borrow_mut()
                .failed
                .insert(key);
        }
        loop {
            let Some(state) = self.states.last_mut() else {
                if let Some(record) = self.record.take() {
                    self.matcher
                        .record_repeat_results(self.repeat, self.start, record);
                }
                return None;
            };
            if !self.greedy && !state.reported {
                if let Some(position) = self.report() {
                    return Some(position);
                }
                continue;
            }
            let state = self.states.last_mut().unwrap();
            if state.count > 0 && !state.last {
                match &self.later {
                    Later::All => {}
                    Later::None => state.last = true,
                    Later::EmptyOnly(writes) => {
                        // the successors would all have been reached
                        // already, but for the empty repetition that ends
                        // the search here, which the operand reports last
                        state.last = true;
                        if state.count < self.max {
                            let (position, count) = (state.position, state.count + 1);
                            let undo = self.matcher.trail_mark();
                            for write in writes {
                                match write {
                                    EmptyWrite::Set(group_nr) => {
                                        self.matcher.capture_empty(*group_nr, position)
                                    }
                                    EmptyWrite::Unbind(groups) => {
                                        unbind_groups(self.matcher, groups);
                                    }
                                }
                            }
                            self.states.push(State {
                                position,
                                count,
                                operand: None,
                                reported: false,
                                last: true,
                                undo: Some(undo),
                            });
                            continue;
                        }
                    }
                }
            }
            let state = self.states.last_mut().unwrap();
            if state.count < self.max && !state.last {
                let (position, count) = (state.position, state.count + 1);
                let operand = state
                    .operand
                    .get_or_insert_with(|| self.operation.matches_iter(self.matcher, position));
                #[cfg(test)]
                STEPS.with(|steps| steps.set(steps.get() + 1));
                match operand.next() {
                    Some(end) if end == position && count > self.min => {
                        // an empty repetition beyond the minimum: the last
                        // one (see the type's comment). It reaches a
                        // position already reached, so when only the answer
                        // counts it adds nothing.
                        if self.matcher.answer_only() {
                            continue;
                        }
                        self.states.push(State {
                            position: end,
                            count,
                            operand: None,
                            reported: false,
                            last: true,
                            undo: None,
                        });
                        continue;
                    }
                    Some(end) => {
                        let entry = self.enter(end, count);
                        if entry != Entry::Skip {
                            self.states.push(State {
                                position: end,
                                count,
                                operand: None,
                                reported: false,
                                last: entry == Entry::ReportOnly,
                                undo: None,
                            });
                        }
                        continue;
                    }
                    None => {
                        // exhausted: it has taken back its captures
                        self.states.last_mut().unwrap().operand = None;
                    }
                }
            }
            // the successors are done
            if self.greedy && !self.states.last().unwrap().reported {
                if let Some(position) = self.report() {
                    return Some(position);
                }
            }
            self.settle();
            if let Some(mark) = self.states.pop().unwrap().undo {
                self.matcher.undo_to(mark);
            }
        }
    }
}

// What a repetition reported from a position the first time, reported again
// (`ReMatcher::repeat_results`): each position with the captures the
// repetition had made on the way to it.
struct Replay<'a> {
    matcher: &'a ReMatcher<'a>,
    results: Rc<[RepeatResult]>,
    next: usize,
    // the capture log's length when it started
    mark: usize,
    zero_groups: Option<Vec<usize>>,
}

impl Iterator for Replay<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        self.matcher.undo_to(self.mark);
        let result = self.results.get(self.next)?;
        self.next += 1;
        if result.zero {
            if let Some(groups) = &self.zero_groups {
                unbind_groups(self.matcher, groups);
            }
        }
        for capture in result.captures.iter() {
            self.matcher.restore_capture(capture);
        }
        Some(result.position)
    }
}

#[cfg(test)]
mod tests {
    use crate::re_matcher::NO_REPEAT_RESULTS;
    use crate::Regex;

    // The work of the search grows with the input no faster than the square
    // of its length for searches (every start position) and linearly for a
    // whole-string match, for the family of repetitions of operands that can
    // match the empty string, counted in operand matches asked for
    // (`STEPS`), so that the bound does not depend on the machine. Before the
    // shortcuts a search was cubic (`(a*)*b` over 800 a's: more than 5 s).
    #[test]
    fn test_nested_repetitions_take_at_most_quadratic_work() {
        use super::STEPS;
        let steps = |f: &dyn Fn(&str) -> bool, n: usize, suffix: &str| {
            let input = format!("{}{suffix}", "a".repeat(n));
            STEPS.with(|steps| steps.set(0));
            f(&input);
            STEPS.with(|steps| steps.get()).max(1)
        };
        for (pattern, suffix) in [
            ("(a*)*b", "cb"),
            ("(a+)+b", "cb"),
            ("(a|aa)*b", "cb"),
            ("((a*)*)*b", "cb"),
            ("(a*)*$", "c"),
            ("(\\w*)*@", "-@"),
            ("([a-z]*)*x", "-x"),
            ("(a*){2,}b", "cb"),
            ("(?:a*)*b", "cb"),
            ("(a?)*b", "cb"),
        ] {
            let regex = Regex::xpath(pattern, "").unwrap();
            type Op<'a> = (&'a str, Box<dyn Fn(&str) -> bool + 'a>, f64);
            let ops: Vec<Op> = vec![
                ("is_match", Box::new(|s: &str| regex.is_match(s)), 4.5),
                (
                    "is_full_match",
                    Box::new(|s: &str| regex.is_full_match(s)),
                    2.5,
                ),
                (
                    "replace_all",
                    Box::new(|s: &str| regex.replace_all(s, "[$1]").is_ok()),
                    4.5,
                ),
                (
                    "tokenize",
                    Box::new(|s: &str| regex.tokenize(s).map(|t| t.count()).is_ok()),
                    4.5,
                ),
            ];
            for (name, op, ratio) in &ops {
                let (small, large) = (
                    steps(op.as_ref(), 200, suffix),
                    steps(op.as_ref(), 400, suffix),
                );
                let growth = large as f64 / small as f64;
                assert!(
                    growth <= *ratio,
                    "{pattern:?} {name}: {small} → {large} operand matches when the input doubles"
                );
            }
        }
    }

    // Every operation, with and without the shortcuts of the search for
    // repetitions that are not nested (`Settled`) and for operands closed
    // under repetition (`Repeat::later_successors`): the same answers, the
    // same captures, the same matches in the same order.
    #[test]
    fn test_shortcuts_change_nothing() {
        use crate::re_matcher::NO_SHORTCUTS;
        const PIECES: &[&str] = &[
            "a",
            "b",
            "(a*)",
            "((a*)*)",
            "(a+)",
            "((a)*)",
            "(?:a*)",
            "((a|b)*)",
            "(a*b*)",
            "((a)+)",
            "([ab]*)",
            "(a|)",
            "(a?)",
            "((a*)+)",
            "(?:(a)*)",
            "(a|aa)",
            "(",
            ")",
            "|",
            "$",
            "^",
            "((a|b)*?)",
            "((a*)*?)",
            "(a*?)",
            "((a|ab)*)",
            "((ab|a)+?)",
        ];
        const QUANTS: &[&str] = &[
            "", "", "*", "*", "+", "?", "{2}", "{0,2}", "{1,}", "{2,}", "*?", "+?", "{1,}?",
        ];
        const INPUTS: &[&str] = &[
            "", "a", "b", "ab", "aab", "abab", "baab", "aaaa", "aaab", "abba", "aaaab", "abaab",
            "bbaab", "aabba",
        ];
        let mut seed: u64 = 20261018;
        let mut next = |n: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % n
        };
        let mut compiled = 0;
        for _ in 0..12000 {
            let pattern: String = (0..1 + next(4))
                .map(|_| {
                    format!(
                        "{}{}",
                        PIECES[next(PIECES.len())],
                        QUANTS[next(QUANTS.len())]
                    )
                })
                .collect();
            let Ok(regex) = Regex::xpath(&pattern, "") else {
                continue;
            };
            compiled += 1;
            let count = pattern.matches('(').count() - pattern.matches("(?").count();
            let groups = (1..=count.min(9))
                .map(|i| format!("${i}"))
                .collect::<Vec<_>>()
                .join("|");
            let template = format!("[{groups}]");
            let run = |no: bool| {
                NO_SHORTCUTS.with(|cell| cell.set(no));
                let out = INPUTS
                    .iter()
                    .map(|input| {
                        (
                            regex.is_match(input),
                            regex.is_full_match(input),
                            regex.replace_all(input, &template),
                            regex.tokenize(input).map(|t| t.collect::<Vec<_>>()),
                            regex
                                .analyze(input)
                                .map(|a| format!("{:?}", a.collect::<Vec<_>>())),
                        )
                    })
                    .collect::<Vec<_>>();
                NO_SHORTCUTS.with(|cell| cell.set(false));
                out
            };
            assert_eq!(run(false), run(true), "{pattern:?}");
        }
        assert!(compiled > 6000, "{compiled}");
    }

    // Every operation, with and without the reuse of repetition results
    // (`ReMatcher::repeat_results`): the same answers, the same captures.
    #[test]
    fn test_reused_results_change_nothing() {
        // groups inside repetitions inside counted repetitions, where an
        // enclosing repetition comes back to a position below its minimum
        const PIECES: &[&str] = &[
            "a",
            "b",
            "(a|)",
            "(|b)",
            "(a?)",
            "(a*)",
            "(b?|a)",
            "((a|b)*)",
            "(a(b)?)",
            "(?:a|)",
            "()",
            "[ab]",
            ".",
            "(",
            ")",
            "|",
            "((a)*b?){2}",
            "(?:((a|b)*)|b){2,3}",
            "((a*)(b*)){2}",
            "(?:a|aa|((b)|bb)*c)",
            "(?:a|((a|b)*))",
            "((b|bb)*)",
            "(?:aa|a)",
            "(?:(x|(a))*b)",
        ];
        const QUANTS: &[&str] = &[
            "", "", "?", "*", "+", "{2}", "{0,2}", "{1,2}", "{1,3}", "{2,3}", "*?", "+?", "{2,}?",
        ];
        const INPUTS: &[&str] = &[
            "", "a", "ab", "aab", "abab", "baab", "aaaa", "abba", "aabbc", "abbcabc", "aabcbbc",
            "xab", "aaab", "aabab",
        ];
        let mut seed: u64 = 20261017;
        let mut next = |n: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % n
        };
        let mut compiled = 0;
        for _ in 0..12000 {
            let pattern: String = (0..1 + next(5))
                .map(|_| {
                    format!(
                        "{}{}",
                        PIECES[next(PIECES.len())],
                        QUANTS[next(QUANTS.len())]
                    )
                })
                .collect();
            let Ok(regex) = Regex::xpath(&pattern, "") else {
                continue;
            };
            compiled += 1;
            let count = pattern.matches('(').count() - pattern.matches("(?").count();
            let groups = (1..=count.min(9))
                .map(|i| format!("${i}"))
                .collect::<Vec<_>>()
                .join("|");
            let template = format!("[{groups}]");
            let run = |no_reuse: bool| {
                NO_REPEAT_RESULTS.with(|no| no.set(no_reuse));
                let out = INPUTS
                    .iter()
                    .map(|input| {
                        (
                            regex.is_match(input),
                            regex.is_full_match(input),
                            regex.replace_all(input, &template),
                            regex.tokenize(input).map(|t| t.collect::<Vec<_>>()),
                            regex
                                .analyze(input)
                                .map(|a| format!("{:?}", a.collect::<Vec<_>>())),
                        )
                    })
                    .collect::<Vec<_>>();
                NO_REPEAT_RESULTS.with(|no| no.set(false));
                out
            };
            assert_eq!(run(false), run(true), "{pattern:?}");
        }
        assert!(compiled > 6000, "{compiled}");
    }

    #[test]
    fn test_repeat_simple() {
        let regex = Regex::xpath(r#"a*"#, "").unwrap();
        let op = regex.path("0");

        // unambiguous repeat
        let matches = regex.matcher("a").operation_matches(op.clone());
        assert_eq!(matches, vec!["a"]);

        let matches = regex.matcher("").operation_matches(op);
        assert_eq!(matches, vec![""]);
    }

    #[test]
    fn test_repeat_choice() {
        let regex = Regex::xpath(r#"(?:a|b)*"#, "").unwrap();
        let op = regex.path("0");

        let matches = regex.matcher("").operation_matches(op.clone());
        assert_eq!(matches, vec![""]);

        let matches = regex.matcher("a").operation_matches(op.clone());
        assert_eq!(matches, vec!["a", ""]);
        let matches = regex.matcher("aba").operation_matches(op.clone());
        assert_eq!(matches, vec!["aba", "ab", "a", ""]);

        let matches = regex.matcher("bab").operation_matches(op);
        assert_eq!(matches, vec!["bab", "ba", "b", ""]);
    }

    #[test]
    fn test_repeat_choice_with_option_first() {
        let regex = Regex::xpath(r#"(?:a?|b)*"#, "").unwrap();
        let op = regex.path("0");

        let matches = regex.matcher("bab").operation_matches(op);

        assert!(matches.contains(&"bab".to_string()));
    }

    #[test]
    fn test_repeat_option() {
        let regex = Regex::xpath(r#"(?:a?)*"#, "").unwrap();
        let op = regex.path("0");

        let matches = regex.matcher("aaa").operation_matches(op);

        assert!(matches.contains(&"aaa".to_string()));
    }

    #[test]
    fn test_repeat_choice_with_option_last() {
        let regex = Regex::xpath(r#"(?:b|a?)*"#, "").unwrap();
        let op = regex.path("0");

        let matches = regex.matcher("bab").operation_matches(op);
        assert_eq!(matches[0], "bab");
    }

    // #[test]
    // fn test_repeat_with_atom_behind() {
    //     let regex = Regex::xpath(r#"^(.*)+B"#, "").unwrap();
    //     let op = regex.path("0");

    //     let matches = regex.matcher("AB").operation_matches(op);
    //     assert_eq!(matches, vec!["AB"]);
    // }
}
