use crate::re_compiler::Error;

/// Regular expressions can be compiled for XSD, or following
/// the XPath superset.
///
/// Only the XPath superset is currently properly tested.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub(crate) enum Language {
    /// Follow the XML Schema definition of regular expressions only,
    /// don't allow XPath extensions
    Xsd,
    /// Follow the XPath 3.1 definition of regular expressions, which
    /// includes some additional features.
    XPath,
    /// Follow the XPath 2.0 definition of regular expressions (F&O 1.0
    /// Second Edition §7.6.1): the XPath extensions without the ones F&O 3.0
    /// added — no non-capturing groups, only the flags `s`, `m`, `i` and `x`
    /// — and with `.` excluding only the newline when `s` is not given.
    XPath20,
}

impl Language {
    /// Whether the XPath extensions of XML Schema's regular expressions
    /// apply: `^` and `$` anchors, the `\$` escape, back-references,
    /// reluctant quantifiers.
    pub(crate) fn is_xpath(self) -> bool {
        matches!(self, Language::XPath | Language::XPath20)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ReFlags {
    case_independent: bool,
    multi_line: bool,
    single_line: bool,
    allow_whitespace: bool,
    literal: bool,
    language: Language,
    debug: bool,                     // flags = ';g'
    allow_unknown_block_names: bool, // flags = ';k'
}

impl ReFlags {
    pub(crate) fn new(flags: &str, language: Language) -> Result<Self, Error> {
        let mut r = Self {
            case_independent: false,
            multi_line: false,
            single_line: false,
            allow_whitespace: false,
            literal: false,
            language,
            debug: false,
            allow_unknown_block_names: false,
        };

        let mut chars = flags.chars();

        for c in chars.by_ref() {
            if c == ';' {
                if language == Language::XPath20 {
                    // F&O 1.0 §7.6.1.1: "If there are characters present that
                    // are not defined here as flags, then an error is raised
                    // [err:FORX0001]"; it defines `s`, `m`, `i` and `x`.
                    return Err(Error::InvalidFlags(
                        "Unrecognized flag ';' (XPath 2.0 defines only s, m, i and x)".to_string(),
                    ));
                }
                break;
            }
            match c {
                'i' => {
                    r.case_independent = true;
                }
                'm' => {
                    r.multi_line = true;
                }
                's' => {
                    r.single_line = true;
                }
                'q' => {
                    r.literal = true;
                    if language != Language::XPath {
                        return Err(Error::InvalidFlags(
                            "'q' flag requires XPath 3.0 to be enabled".to_string(),
                        ));
                    }
                }
                'x' => {
                    r.allow_whitespace = true;
                }
                _ => {
                    return Err(Error::InvalidFlags(format!("Unrecognized flag '{}'", c)));
                }
            }
        }
        // after ';'
        for c in chars {
            match c {
                'g' => {
                    r.debug = true;
                }
                'k' => {
                    r.allow_unknown_block_names = true;
                }
                'K' => {
                    r.allow_unknown_block_names = false;
                }
                _ => {
                    return Err(Error::InvalidFlags(format!("Unrecognized flag '{}'", c)));
                }
            }
        }
        Ok(r)
    }

    pub(crate) fn is_case_independent(&self) -> bool {
        self.case_independent
    }

    pub(crate) fn is_multi_line(&self) -> bool {
        self.multi_line
    }

    pub(crate) fn is_single_line(&self) -> bool {
        self.single_line
    }

    pub(crate) fn is_allow_whitespace(&self) -> bool {
        self.allow_whitespace
    }

    pub(crate) fn is_literal(&self) -> bool {
        self.literal
    }

    pub(crate) fn language(&self) -> Language {
        self.language
    }

    // This appears to do nothing in the Java original; it's triggered
    // by the 'g' flag
    // pub(crate) fn is_debug(&self) -> bool {
    //     self.debug
    // }

    // Whether a block escape with an unrecognized block name denotes the set
    // of all characters (XSD 1.1 Part 2 §G.4.2.4) instead of being an error;
    // the Java original also gives a warning, which we cannot do.
    pub(crate) fn is_allow_unknown_block_names(&self) -> bool {
        self.allow_unknown_block_names
    }
}
