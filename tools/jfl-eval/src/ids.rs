//! The only strings a report may carry besides fixed metric names: arm and task ids.
//!
//! An [`Id`] can only be built by [`Id::parse`], which accepts lowercase ASCII letters, digits and
//! single hyphens, 1 to 64 characters. A prompt, a path or a line of Java cannot pass that shape
//! (no spaces, slashes, dots or upper case), so a report built from `Id`s and numbers cannot leak
//! one. Pinned by: `tests/report.rs::should_leave_out_every_planted_string_when_the_input_carries_them`.

use std::fmt;

use serde::{Serialize, Serializer};

/// The longest id accepted.
pub const MAX_ID_LEN: usize = 64;

/// An arm or task id: `[a-z0-9]+(-[a-z0-9]+)*`, at most [`MAX_ID_LEN`] characters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(String);

impl Id {
    /// The id, if `s` has the shape; `None` otherwise.
    pub fn parse(s: &str) -> Option<Self> {
        let shaped = !s.is_empty()
            && s.len() <= MAX_ID_LEN
            && !s.starts_with('-')
            && !s.ends_with('-')
            && !s.contains("--")
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        shaped.then(|| Self(s.to_string()))
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

/// The separator between the arm and the task in a generated Harbor task's directory name
/// (`<arm>__<task>`), which Harbor records as `task_name`. An id never holds `_`, so the split is
/// unambiguous.
pub const ARM_TASK_SEPARATOR: &str = "__";

/// The arm and task of a Harbor `task_name` written by [`crate::harbor`]; `None` for any other.
pub fn split_task_name(task_name: &str) -> Option<(Id, Id)> {
    let (arm, task) = task_name.split_once(ARM_TASK_SEPARATOR)?;
    Some((Id::parse(arm)?, Id::parse(task)?))
}

/// Whether `s` is a full Git commit id: 40 lowercase hexadecimal digits.
pub fn is_commit(s: &str) -> bool {
    s.len() == 40
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_accept_an_id_when_it_is_lowercase_words_joined_by_hyphens() {
        // given
        let s = "fix-ledger-rounding-2";
        // when
        let id = Id::parse(s);
        // then
        assert_eq!(id.map(|i| i.to_string()), Some(s.to_string()));
    }

    #[test]
    fn should_refuse_an_id_when_it_could_carry_text_or_a_path() {
        // given
        let bad = [
            "",
            "Fix",
            "a b",
            "src/Main.java",
            "a.b",
            "-a",
            "a-",
            "a--b",
            "a_b",
            &"a".repeat(65),
        ];
        // when
        let accepted: Vec<&str> = bad
            .iter()
            .copied()
            .filter(|s| Id::parse(s).is_some())
            .collect();
        // then
        assert!(accepted.is_empty(), "{accepted:?}");
    }

    #[test]
    fn should_split_arm_and_task_when_the_name_was_generated() {
        // given
        let name = "jdtls-proxy__fix-rounding";
        // when
        let split = split_task_name(name);
        // then
        let (arm, task) = split.unwrap();
        assert_eq!(
            (arm.as_str(), task.as_str()),
            ("jdtls-proxy", "fix-rounding")
        );
    }

    #[test]
    fn should_refuse_a_task_name_when_it_was_not_generated() {
        // given
        let names = ["hello-world", "org/name", "a__b__c", "A__b"];
        // when
        let split: Vec<bool> = names.iter().map(|n| split_task_name(n).is_some()).collect();
        // then
        assert_eq!(split, vec![false, false, false, false]);
    }

    #[test]
    fn should_accept_a_commit_when_it_is_forty_lowercase_hex_digits() {
        // given
        let good = "3cb627394b95d3ae605f31e0d694edf508cf4b7b";
        // when
        // then
        assert!(is_commit(good));
        assert!(!is_commit("3CB627394B95D3AE605F31E0D694EDF508CF4B7B"));
        assert!(!is_commit("main"));
    }
}
