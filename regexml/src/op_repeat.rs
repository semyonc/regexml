use std::rc::Rc;

use ahash::{HashMap, HashMapExt, HashSet, HashSetExt};

use crate::{
    character_class::CharacterClass,
    operation::{
        captures, groups_of, unbind_groups, Operation, OperationControl, RepeatOperation,
        RestoreCaptures, MATCHES_ZLS_ANYWHERE,
    },
    re_flags::ReFlags,
    re_matcher::{ReMatcher, RepeatResult},
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
}

impl Repeat {
    pub(crate) fn new(operation: Operation, min: usize, max: usize, greedy: bool) -> Self {
        Self {
            operation: Box::new(operation),
            min,
            max,
            greedy,
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
            record: matcher.may_reuse_repeat_results().then(Vec::new),
            start: position,
            mark: matcher.trail_mark(),
            matcher,
            operation: self.operation.as_ref(),
            min: self.min,
            max: self.max,
            greedy: self.greedy,
            states: vec![State {
                position,
                count: 0,
                operand: None,
                reported: false,
                last: false,
            }],
            zero_groups,
            unbound: None,
            backrefs,
            entered: HashMap::new(),
            entered_below_min: HashSet::new(),
            reported: HashSet::new(),
        })
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
    // whether it was reached with an empty repetition beyond the minimum,
    // after which there is no further repetition
    last: bool,
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
}

// A state's position, and what the groups hold when the program has
// back-references (`ReMatcher::backref_snapshot`).
type Key = (usize, Option<Box<[Option<usize>]>>);

impl RepeatIterator<'_> {
    fn key(&self, position: usize) -> Key {
        (
            position,
            self.backrefs.then(|| self.matcher.backref_snapshot()),
        )
    }

    // Whether the state `count` repetitions at `position` is worth entering.
    fn enter(&mut self, position: usize, count: usize) -> bool {
        if count < self.min {
            // below the minimum, the number of repetitions still to do
            // matters too
            let key = (self.key(position), count);
            return self.entered_below_min.insert(key);
        }
        let key = self.key(position);
        match self.entered.get(&key) {
            Some(&fewest) if fewest <= count => false,
            _ => {
                self.entered.insert(key, count);
                true
            }
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
        if !self.reported.insert(key) {
            return None;
        }
        if count == 0 {
            if let Some(groups) = &self.zero_groups {
                self.unbound = Some(unbind_groups(self.matcher, groups));
            }
        }
        if let Some(record) = &mut self.record {
            let captures = if self.matcher.tracks_captures() {
                self.matcher.captures_since(self.mark)
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
            if state.count < self.max && !state.last {
                let (position, count) = (state.position, state.count + 1);
                let operand = state
                    .operand
                    .get_or_insert_with(|| self.operation.matches_iter(self.matcher, position));
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
                        });
                        continue;
                    }
                    Some(end) => {
                        if self.enter(end, count) {
                            self.states.push(State {
                                position: end,
                                count,
                                operand: None,
                                reported: false,
                                last: false,
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
            self.states.pop();
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
