use crate::{
    op_greedy_fixed::{uncovered_ends, IntStepIterator},
    operation::{
        captures, groups_of, unbind_groups, Operation, OperationControl, RepeatOperation,
        RestoreCaptures, Within, MATCHES_ZLS_ANYWHERE,
    },
    re_flags::ReFlags,
    re_matcher::ReMatcher,
};

// Handle a reluctant repetition (with possible min and max) where the size of
// the repeated unit is fixed.
#[derive(Debug, Clone)]
pub(crate) struct ReluctantFixed {
    operation: Box<Operation>,
    pub(crate) min: usize,
    max: usize,
    len: usize,
    // whether it is in the operand of a repetition that is not nested, with
    // only sequences, choices and groups in between (`ReMatcher::covered`)
    covered: bool,
}

impl ReluctantFixed {
    pub(crate) fn new(operation: Operation, min: usize, max: usize, len: usize) -> Self {
        Self {
            operation: operation.into(),
            min,
            max,
            len,
            covered: false,
        }
    }
}

impl OperationControl for ReluctantFixed {
    fn get_match_length(&self) -> Option<usize> {
        if self.min == self.max {
            Some(self.min * self.len)
        } else {
            None
        }
    }

    fn get_minimum_match_length(&self) -> usize {
        self.min * self.operation.get_minimum_match_length()
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

    fn optimize(self, flags: &ReFlags) -> Operation {
        let operation = self.operation.optimize(flags);
        Operation::from(ReluctantFixed {
            operation: operation.into(),
            min: self.min,
            max: self.max,
            len: self.len,
            covered: self.covered,
        })
    }

    fn matches_iter<'a>(
        &'a self,
        matcher: &'a ReMatcher<'a>,
        position: usize,
    ) -> Box<dyn Iterator<Item = usize> + 'a> {
        if self.covered && self.max == usize::MAX {
            // only the answer counts: the order of the ends does not matter
            if let Some(mut seen) = matcher.covered(self as *const Self as usize) {
                let ends = uncovered_ends(
                    &self.operation,
                    matcher,
                    &mut seen,
                    position,
                    self.min,
                    self.len,
                );
                return match ends {
                    Some((first, last)) => {
                        Box::new(IntStepIterator::new(first, self.len as i64, last))
                    }
                    None => Box::new(std::iter::empty()),
                };
            }
        }
        let capturing = matcher.tracks_captures() && captures(&self.operation);
        let iter = Box::new(ReluctantFixedIterator::new(
            self.operation.as_ref(),
            matcher,
            position,
            self.min,
            self.max,
            if capturing && self.min == 0 {
                groups_of(&self.operation)
            } else {
                Vec::new()
            },
        ));
        if capturing {
            // the iterations it has done are kept as it adds one more, and
            // taken back only when it gives up
            RestoreCaptures::wrap(matcher, matcher.trail_mark(), iter)
        } else {
            iter
        }
    }

    fn children(&self) -> Vec<Operation> {
        vec![self.operation.as_ref().clone()]
    }

    fn collect_groups(&self, groups: &mut Vec<usize>) {
        self.operation.collect_groups(groups);
    }

    fn mark_nested(&mut self, within: Within) {
        self.covered = within == Within::TopOperand;
        self.operation.mark_nested(Within::Nested);
    }
}

impl RepeatOperation for ReluctantFixed {
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
        false
    }
}

struct ReluctantFixedIterator<'a> {
    op: &'a Operation,
    matcher: &'a ReMatcher<'a>,
    count: usize,
    min: usize,
    max: usize,
    started: bool,
    pos: usize,
    // with `min` 0 and a capturing operand: its groups, left unmatched while
    // zero iterations are reported (`unbind_groups`), and the capture log's
    // length to go back to before the first iteration is tried
    groups: Vec<usize>,
    unbound: Option<usize>,
}

impl<'a> ReluctantFixedIterator<'a> {
    fn new(
        op: &'a Operation,
        matcher: &'a ReMatcher<'a>,
        position: usize,
        min: usize,
        max: usize,
        groups: Vec<usize>,
    ) -> Self {
        Self {
            op,
            matcher,
            count: 0,
            min,
            max,
            started: false,
            pos: position,
            groups,
            unbound: None,
        }
    }
}

impl Iterator for ReluctantFixedIterator<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.started {
            self.started = true;

            while self.count < self.min {
                let mut it = self.op.matches_iter(self.matcher, self.pos);
                if let Some(next) = it.next() {
                    self.count += 1;
                    self.pos = next;
                } else {
                    return None;
                }
            }
            if self.count == 0 && !self.groups.is_empty() {
                self.unbound = Some(unbind_groups(self.matcher, &self.groups));
            }
            return Some(self.pos);
        }
        if let Some(mark) = self.unbound.take() {
            self.matcher.undo_to(mark);
        }

        if self.count < self.max {
            let mut it = self.op.matches_iter(self.matcher, self.pos);
            if let Some(next) = it.next() {
                self.pos = next;
                self.count += 1;
                return Some(self.pos);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::Regex;

    #[test]
    fn test_repeat_star_relucant() {
        let regex = Regex::xpath(r#"a*?"#, "").unwrap();
        let op = regex.path("0");
        let matcher = regex.matcher("aaaaa");
        let matches = matcher.operation_matches(op);
        assert_eq!(matches, vec!["", "a", "aa", "aaa", "aaaa", "aaaaa"]);
    }

    #[test]
    fn test_repeat_plus_reluctant() {
        let regex = Regex::xpath(r#"a+?"#, "").unwrap();
        let op = regex.path("0");
        let matcher = regex.matcher("aaaaa");
        let matches = matcher.operation_matches(op);
        assert_eq!(matches, vec!["a", "aa", "aaa", "aaaa", "aaaaa"]);
    }

    #[test]
    fn test_repeat_question_reluctant() {
        let regex = Regex::xpath(r#"a??"#, "").unwrap();
        let op = regex.path("0");
        let matcher = regex.matcher("aaaaa");
        let matches = matcher.operation_matches(op);
        assert_eq!(matches, vec!["", "a"]);
    }
}
