use icu_collections::codepointinvlist::CodePointInversionListBuilder;

use crate::{
    character_class::CharacterClass,
    operation::{Operation, OperationControl, MATCHES_ZLS_ANYWHERE, MATCHES_ZLS_NEVER},
    re_flags::ReFlags,
    re_matcher::ReMatcher,
};

/// A match against a fixed string of any length, within a regular expression.
#[derive(Debug, Clone)]
pub(crate) struct Atom {
    pub(crate) atom: Vec<char>,
    len: usize,
}

impl Atom {
    pub(crate) fn new(atom: Vec<char>) -> Self {
        Self {
            len: atom.len(),
            atom,
        }
    }
}

impl OperationControl for Atom {
    fn get_match_length(&self) -> Option<usize> {
        Some(self.len)
    }

    fn matches_empty_string(&self) -> u32 {
        if self.len == 0 {
            MATCHES_ZLS_ANYWHERE
        } else {
            MATCHES_ZLS_NEVER
        }
    }

    fn get_initial_character_class(&self, case_blind: bool) -> CharacterClass {
        if self.len == 0 {
            return CharacterClass::empty();
        }

        if case_blind {
            // Under `i` an atom compares characters by their simple lowercase
            // mappings (`re_matcher::equal_case_blind`), so its first
            // character can be matched by characters that ICU's case closure
            // does not give: the closure leaves out the character itself
            // ('a' gives {'A'}), and 'i' matches U+0130 LATIN CAPITAL LETTER I
            // WITH DOT ABOVE, whose simple lowercase mapping is 'i', though
            // neither is in the other's closure. This class may over-estimate
            // (`OperationControl::get_initial_character_class`), and its only
            // reader is `ReCompiler::no_ambiguity`, which made `a*A` possessive
            // from the closures {'A'} and {'a'} and so rejected "aA". Every
            // character keeps a case-blind atom out of that judgement.
            return CharacterClass::all();
        }
        let mut builder = CodePointInversionListBuilder::new();
        builder.add_char(self.atom[0]);
        CharacterClass::new(builder.build())
    }

    fn optimize(self, _flags: &ReFlags) -> Operation {
        Operation::from(self)
    }

    fn matches_iter<'a>(
        &self,
        matcher: &'a ReMatcher,
        position: usize,
    ) -> Box<dyn Iterator<Item = usize> + 'a> {
        let in_ = &matcher.search;
        if (position + self.len) > in_.len() {
            return Box::new(std::iter::empty());
        }
        let mut in_chars = in_.iter().skip(position);
        let atom_chars = &self.atom;
        if matcher.program.flags.is_case_independent() {
            for atom_char in atom_chars {
                let in_char = in_chars.next().unwrap();
                if !matcher.equal_case_blind(*in_char, *atom_char) {
                    return Box::new(std::iter::empty());
                }
            }
        } else {
            for atom_char in atom_chars {
                let in_char = in_chars.next().unwrap();
                if *in_char != *atom_char {
                    return Box::new(std::iter::empty());
                }
            }
        }
        Box::new(std::iter::once(position + self.len))
    }
}

#[cfg(test)]
mod tests {
    use crate::Regex;

    #[test]
    fn test_atom_case_sensitive() {
        let regex = Regex::xpath("abc", "").unwrap();
        let op = regex.path("0");
        let matches = regex.matcher("abc").operation_matches(op.clone());
        assert_eq!(matches, vec!["abc"]);
        let matches = regex.matcher("a").operation_matches(op.clone());
        assert!(matches.is_empty());
        let matches = regex.matcher("abcd").operation_matches(op.clone());
        assert_eq!(matches, vec!["abc"]);
    }

    #[test]
    fn test_atom_case_insensitive() {
        let regex = Regex::xpath("abc", "i").unwrap();
        let op = regex.path("0");
        let matches = regex.matcher("abc").operation_matches(op.clone());
        assert_eq!(matches, vec!["abc"]);
        let matches = regex.matcher("ABC").operation_matches(op.clone());
        assert_eq!(matches, vec!["ABC"]);
    }
}
