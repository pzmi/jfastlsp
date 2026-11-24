//! The contract suites (ver §5): one per port, parameterised by the implementation under test,
//! run against the testkit fake here and against the real adapter from the adapter's crate.

pub mod endpoint;
pub mod process_runner;
pub mod repo_probe;
pub mod source_tree;
pub mod summary_store;
pub mod syntax_indexer;

/// What a fixture did when a case asked it for a `.java` file whose name is not UTF-8
/// (`repo_probe`, `source_tree`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonUtf8Name {
    /// The file exists under that name.
    Written,
    /// The platform's filesystem refused the name (`EILSEQ`), so no such file exists.
    Refused,
}

/// What this platform's filesystem does with a name that is not UTF-8. APFS, the filesystem of
/// macOS, refuses one with `EILSEQ`; Linux's filesystems hold any bytes but `/` and NUL. A real
/// fixture reports what its filesystem did, and an adapter's contract test asserts that it is this
/// (`crates/jfl-adapter-fs/tests/contract.rs`, `crates/jfl-adapter-git/tests/contract.rs`).
#[cfg(target_os = "macos")]
pub const PLATFORM_NON_UTF8_NAME: NonUtf8Name = NonUtf8Name::Refused;
/// What this platform's filesystem does with a name that is not UTF-8. APFS, the filesystem of
/// macOS, refuses one with `EILSEQ`; Linux's filesystems hold any bytes but `/` and NUL. A real
/// fixture reports what its filesystem did, and an adapter's contract test asserts that it is this
/// (`crates/jfl-adapter-fs/tests/contract.rs`, `crates/jfl-adapter-git/tests/contract.rs`).
#[cfg(not(target_os = "macos"))]
pub const PLATFORM_NON_UTF8_NAME: NonUtf8Name = NonUtf8Name::Written;

/// How many names that are not UTF-8 a case's answer must report after the fixture's `outcome`:
/// one for a written file; none for a refused one, but only where the platform refuses such
/// names, so a refusal can never stand in for the case on a filesystem that holds them.
///
/// Pinned by: `contracts::tests`.
pub fn non_utf8_names(outcome: NonUtf8Name) -> Result<u64, String> {
    match outcome {
        NonUtf8Name::Written => Ok(1),
        NonUtf8Name::Refused if PLATFORM_NON_UTF8_NAME == NonUtf8Name::Refused => Ok(0),
        NonUtf8Name::Refused => Err(
            "the fixture refused a name that is not UTF-8 on a platform whose filesystem holds \
             one"
            .to_owned(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_count_one_name_when_the_fixture_wrote_it() {
        // given
        let outcome = NonUtf8Name::Written;

        // when
        let count = non_utf8_names(outcome);

        // then
        assert_eq!(count, Ok(1));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn should_fail_the_case_when_a_platform_that_holds_any_name_refused_one() {
        // given
        let outcome = NonUtf8Name::Refused;

        // when
        let count = non_utf8_names(outcome);

        // then
        assert!(count.is_err(), "{count:?}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn should_count_no_name_when_the_platform_refused_it() {
        // given
        let outcome = NonUtf8Name::Refused;

        // when
        let count = non_utf8_names(outcome);

        // then
        assert_eq!(count, Ok(0));
    }
}
