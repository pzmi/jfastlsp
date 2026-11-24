//! The languages a summary can be of (IDX-17): the tag every [`FileSummary`](crate::FileSummary)
//! carries, so nothing downstream of the syntax index assumes a file is Java.
//!
//! Which file is which language, and which indexer reads it, is decided elsewhere: by name in
//! `jfl-application`'s `languages` registry, and by the indexer composition wires for each
//! language. This module only names them, with a code that is stable because the summary store
//! writes it.
//!
//! **An enum, not an open code.** Adding a language (Go, Kotlin, Python) adds a variant here, and
//! every exhaustive `match` on [`Language`] then fails to compile until it decides what the new
//! language does: that list of compile errors is the checklist of what a language must bring.
//! Rejected alternative: a numeric code that any crate may mint (`Language(u8)`), which would let
//! a summary of a language nobody wired reach the queries unnoticed.

/// A language the syntax index summarises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Language {
    /// Java source (`.java`).
    Java,
}

impl Language {
    /// Every language, in code order.
    pub const ALL: [Self; 1] = [Self::Java];

    /// The stable code of this language: what the summary store writes.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Java => 0,
        }
    }

    /// The language with this code, if any (a stored summary's field: untrusted).
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Java),
            _ => None,
        }
    }

    /// The language's name in lower case, as goldens and reports write it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Java => "java",
        }
    }

    /// The language with this [`Language::name`], if any.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_round_trip_every_language_when_its_code_or_name_is_read_back() {
        // given
        let all = Language::ALL;
        // when
        let by_code: alloc::vec::Vec<Option<Language>> = all
            .iter()
            .map(|language| Language::from_code(language.code()))
            .collect();
        let by_name: alloc::vec::Vec<Option<Language>> = all
            .iter()
            .map(|language| Language::from_name(language.name()))
            .collect();
        // then
        let want: alloc::vec::Vec<Option<Language>> = all.iter().copied().map(Some).collect();
        assert_eq!(by_code, want);
        assert_eq!(by_name, want);
        assert_eq!(Language::from_code(u8::MAX), None);
        assert_eq!(Language::from_name("Java"), None);
    }
}
