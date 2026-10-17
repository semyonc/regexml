#[cfg(test)]
use crate::operation::Operation;
use crate::re_compiler::ReCompiler;
use crate::re_flags::Language;
use crate::re_flags::ReFlags;
use crate::re_matcher::ReMatcher;
use crate::re_program::ReProgram;

pub use crate::analyze_string::AnalyzeIter;
pub use crate::re_compiler::Error;

/// A XML-style regular expression.
#[derive(Debug)]
pub struct Regex {
    re_program: ReProgram,
    matches_empty_string: bool,
}

impl Regex {
    fn new(re: &str, flags: &str, language: Language) -> Result<Self, Error> {
        let re_flags = ReFlags::new(flags, language)?;
        let pattern = re.chars().collect();
        let re_compiler = ReCompiler::new(pattern, re_flags);
        let re_program = re_compiler.compile()?;
        // we need to check if the regex matches the empty string
        let mut matcher = ReMatcher::new_for_answer(&re_program, "");
        let matches_empty_string = matcher.is_match();
        Ok(Self {
            re_program,
            matches_empty_string,
        })
    }

    /// Create a regular expression from a string, using XPath 3.1 rules.
    ///
    /// A block escape with a block name that is not recognized, such as
    /// `\p{IsUnknown}`, is a syntax error, as F&O 3.1 §5.6.1.5 requires; the
    /// `;k` flag described at [`Regex::xsd`] makes it denote all characters
    /// here too. F&O 3.1 §5.6.2 makes a `;` in the flags an error (FORX0001),
    /// so a caller that passes on flags given by a user should not pass on
    /// `;k`.
    pub fn xpath(re: &str, flags: &str) -> Result<Self, Error> {
        Self::new(re, flags, Language::XPath)
    }

    /// Create a regular expression from a string, using XPath 2.0 rules.
    ///
    /// This is the dialect of XQuery 1.0 and XPath 2.0 Functions and
    /// Operators (Second Edition), §7.6.1: the one of [`Regex::xpath`]
    /// without what F&O 3.0 added to it.
    ///
    /// - The flags are `s`, `m`, `i` and `x`, in any order and repeated
    ///   (§7.6.1.1: "If there are characters present that are not defined here
    ///   as flags, then an error is raised \[err:FORX0001\]"). Any other flag —
    ///   `q`, or a `;` with the flags after it — is [`Error::InvalidFlags`].
    /// - There are no non-capturing groups: a `(` outside a character class
    ///   always opens a capturing group, and a `?` after it would be a
    ///   quantifier with nothing to apply to, so a pattern in which an
    ///   unescaped `(` outside a class is followed by `?` — after the
    ///   whitespace removal of the `x` flag — is an [`Error::Syntax`]
    ///   (FORX0002).
    /// - Without the `s` flag, `.` matches every character except the newline
    ///   #xA (§7.6.1.1: "If the s flag is not specified, the meta-character .
    ///   matches any character except a newline (#x0A) character"), where the
    ///   later rule of [`Regex::xpath`] also excludes the carriage return #xD.
    /// - The `x` flag removes exactly the four whitespace characters #x9, #xA,
    ///   #xD and #x20 outside character classes.
    ///
    /// Flags are checked before the pattern, so a call with both wrong reports
    /// the flags. An unknown block name such as `\p{IsUnknown}` is a syntax
    /// error, with no `;k` to change that.
    ///
    /// ```
    /// use regexml_fork::{Error, Regex};
    ///
    /// assert!(matches!(Regex::xpath20("(?:a)", ""), Err(Error::Syntax(_))));
    /// assert!(matches!(Regex::xpath20("a", "q"), Err(Error::InvalidFlags(_))));
    /// assert!(Regex::xpath20("^a.b$", "").unwrap().is_match("a\rb"));
    /// assert!(!Regex::xpath("^a.b$", "").unwrap().is_match("a\rb"));
    /// ```
    pub fn xpath20(re: &str, flags: &str) -> Result<Self, Error> {
        Self::new(re, flags, Language::XPath20)
    }

    /// Create a regular expression from a string, using XML Schema 1.1 rules.
    ///
    /// A block escape with a block name that is not recognized, such as
    /// `\p{IsUnknown}`, is a syntax error by default. XSD 1.1 Part 2 §G.4.2.4
    /// lets it denote the set of all characters instead, for `\p{IsX}` and
    /// `\P{IsX}` alike; ask for that by ending the flags with `;k`, as in
    /// Saxon:
    ///
    /// ```
    /// use regexml_fork::Regex;
    ///
    /// assert!(Regex::xsd(r"\p{IsUnknown}", "").is_err());
    /// let regex = Regex::xsd(r"\p{IsUnknown}", ";k").unwrap();
    /// assert!(regex.is_match("a"));
    /// // the other flags go before the `;`
    /// assert!(Regex::xsd(r"\P{IsUnknown}", "i;k").unwrap().is_match("a"));
    /// ```
    pub fn xsd(re: &str, flags: &str) -> Result<Self, Error> {
        Self::new(re, flags, Language::Xsd)
    }

    /// Returns `true` if the argument matches this regular expression.
    pub fn is_match(&self, haystack: &str) -> bool {
        let mut matcher = ReMatcher::new_for_answer(&self.re_program, haystack);
        matcher.is_match()
    }

    /// Returns `true` if the whole argument matches this regular expression.
    ///
    /// This is the matching operation of XML Schema's `pattern` facet: XSD
    /// "implicitly anchors all regular expressions at the head and tail", so
    /// the expression must account for the entire input. [`Regex::is_match`],
    /// by contrast, searches for a match anywhere inside the haystack, which
    /// is the XPath `fn:matches` semantics.
    ///
    /// Every way the expression can match from the start of the haystack is
    /// tried until one ends at its end, so the alternative or repetition count
    /// a search would settle on first does not decide the answer. In the XSD
    /// dialect `^` and `$` are ordinary characters (XML Schema 1.1 Part 2
    /// §G.3); in the XPath dialect they keep their meaning as anchors.
    ///
    /// ```
    /// use regexml_fork::Regex;
    ///
    /// let regex = Regex::xsd("a|ab", "").unwrap();
    /// assert!(regex.is_full_match("ab"));
    /// assert!(!regex.is_full_match("abc"));
    /// assert!(regex.is_match("abc"));
    ///
    /// assert!(Regex::xsd("^a$", "").unwrap().is_full_match("^a$"));
    /// ```
    pub fn is_full_match(&self, haystack: &str) -> bool {
        // Captures cannot change the answer unless there are back-references,
        // which `new_for_answer` keeps track of.
        let mut matcher = ReMatcher::new_for_answer(&self.re_program, haystack);
        matcher.is_full_match()
    }

    // returns an error if this regex is known to match an empty string.
    // caches the last result so it doesn't have to do the match again.
    fn check_matches_empty_string(&self) -> Result<(), Error> {
        if !self.matches_empty_string {
            Ok(())
        } else {
            Err(Error::MatchesEmptyString)
        }
    }

    /// Returns a string with all pieces matching this regular expression replaced
    /// by the replacement.
    pub fn replace_all(&self, haystack: &str, replacement: &str) -> Result<String, Error> {
        self.check_matches_empty_string()?;

        let mut matcher = self.matcher(haystack);
        let replacement: Vec<char> = replacement.chars().collect();
        matcher
            .replace(&replacement)
            .map(|chars| chars.into_iter().collect())
    }

    /// Returns an iterator of the input string tokenized by the regular expression.
    pub fn tokenize<'a>(&'a self, haystack: &str) -> Result<TokenIter<'a>, Error> {
        // if we input the empty string, we should return no tokens
        if haystack.is_empty() {
            return Ok(TokenIter {
                matcher: ReMatcher::new_without_captures(&self.re_program, haystack),
                prev_end: None,
            });
        }
        self.check_matches_empty_string()?;

        Ok(TokenIter {
            matcher: ReMatcher::new_without_captures(&self.re_program, haystack),
            prev_end: Some(0),
        })
    }

    pub(crate) fn matcher(&self, search: &str) -> ReMatcher<'_> {
        ReMatcher::new(&self.re_program, search)
    }

    #[cfg(test)]
    pub(crate) fn path(&self, s: &str) -> Operation {
        self.re_program.path(s)
    }

    /// Use this regular expression to analyze an input string, The resulting
    /// vector provides both the matching and non-matching substrings. It also
    /// provides access to matched subgroups.
    pub fn analyze<'a>(&'a self, haystack: &str) -> Result<AnalyzeIter<'a>, Error> {
        self.check_matches_empty_string()?;
        Ok(AnalyzeIter::new(
            &self.re_program.pattern,
            self.matcher(haystack),
        ))
    }

    // TODO: continue translating ARegexIterator
    // this also has an isMatching protocol, and a processMatchingSubstring story
    // and a computeNestingTable story too. Need to read more into how this is
    // actually used. - it seems vastly complicated.
}

#[derive(Debug)]
pub struct TokenIter<'a> {
    matcher: ReMatcher<'a>,
    prev_end: Option<usize>,
}

impl Iterator for TokenIter<'_> {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(prev_end) = self.prev_end {
            if self.matcher.matches(prev_end) {
                let start = self.matcher.get_paren_start(0).unwrap();
                let current = self.matcher.search[prev_end..start].iter().collect();
                self.prev_end = self.matcher.get_paren_end(0);
                Some(current)
            } else {
                let current = self.matcher.search[prev_end..].iter().collect();
                self.prev_end = None;
                Some(current)
            }
        } else {
            None
        }
    }
}
