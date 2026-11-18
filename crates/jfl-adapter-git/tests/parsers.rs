//! Property tests of the parsers of `git`'s output and of the Git files the hot path reads (ver
//! §8, plan §11): no input makes one panic, and what `git` prints for a set of paths parses back to
//! that set.

use jfl_adapter_git::parse::{head, java_names, names, object_line, status, three_paths};
use jfl_application::HeadState;
use proptest::prelude::*;

/// Path components as Git may print them: spaces, dots, non-ASCII, no `/` or NUL.
fn component() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 ._é-]{1,12}".prop_filter("not . or ..", |c| c != "." && c != "..")
}

fn path() -> impl Strategy<Value = String> {
    prop::collection::vec(component(), 1..5).prop_map(|parts| parts.join("/"))
}

proptest! {
    #[test]
    fn should_never_panic_when_parsing_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        // given
        let input = &bytes;

        // when
        let parsed = (
            names(input).is_ok(),
            java_names(input).is_ok(),
            status(input).is_ok(),
            object_line(input).is_ok(),
            three_paths(input).is_ok(),
            head(input),
        );

        // then
        prop_assert!(parsed.0 || !input.is_empty(), "empty output is a valid empty list");
    }

    #[test]
    fn should_parse_back_every_path_when_names_are_nul_terminated(paths in prop::collection::vec(path(), 0..20)) {
        // given
        let mut output = Vec::new();
        for p in &paths {
            output.extend_from_slice(p.as_bytes());
            output.push(0);
        }

        // when
        let parsed = names(&output).unwrap();

        // then
        let mut expected = paths.clone();
        expected.sort_unstable();
        expected.dedup();
        let got: Vec<&str> = parsed.paths.iter().map(|p| p.as_str()).collect();
        prop_assert_eq!(got, expected);
        prop_assert_eq!(parsed.unrepresentable, 0);
    }

    #[test]
    fn should_keep_each_path_whole_when_status_entries_hold_spaces(paths in prop::collection::vec(path(), 1..10)) {
        // given
        let mut output = Vec::new();
        for p in &paths {
            output.extend_from_slice(b"1 .M N... 100644 100644 100644 0000 0000 ");
            output.extend_from_slice(p.as_bytes());
            output.push(0);
        }

        // when
        let parsed = status(&output).unwrap();

        // then
        let mut expected = paths.clone();
        expected.sort_unstable();
        expected.dedup();
        let got: Vec<&str> = parsed.working.paths.iter().map(|p| p.as_str()).collect();
        prop_assert_eq!(got, expected);
    }

    #[test]
    fn should_never_name_a_ref_outside_refs_when_head_is_symbolic(bytes in prop::collection::vec(any::<u8>(), 0..128)) {
        // given
        let mut file = b"ref: ".to_vec();
        file.extend_from_slice(&bytes);

        // when
        let parsed = head(&file);

        // then
        if let HeadState::Symbolic(name) = parsed {
            prop_assert!(name.starts_with("refs/"));
            prop_assert!(name.split('/').all(|c| !c.is_empty() && c != "." && c != ".."));
        }
    }
}
