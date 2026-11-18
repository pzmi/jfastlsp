//! Parsers of `git`'s output and of the small Git files the hot path reads: total functions over
//! bytes that refuse what is not in the form asked for, never panic and never index (plan §11).
//! Every path-naming command runs with `-z`, so a path is the bytes between two NULs, verbatim:
//! no quoting to undo and no newline to confuse. The input is already bounded by the run's output
//! limit (`cli::Git::run`).
//!
//! Pinned by `parse::tests` and the property tests in `tests/parsers.rs`.

use jfl_application::source_tree::JAVA_EXTENSION;
use jfl_application::{HeadState, ObjectId};
use jfl_domain::RelPath;

/// The ref `HEAD` names in a repository whose refs are in the reftable format: a placeholder Git
/// writes so that older versions refuse the repository.
pub const REFTABLE_MARKER: &str = "refs/heads/.invalid";

/// Output that is not in the form asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Malformed;

/// The NUL-terminated records of `-z` output. Output that does not end in a NUL (a record cut
/// short) is refused.
pub fn records(bytes: &[u8]) -> Result<impl Iterator<Item = &[u8]>, Malformed> {
    let body = match bytes.split_last() {
        None => &[][..],
        Some((0, body)) => body,
        Some(_) => return Err(Malformed),
    };
    let empty = bytes.is_empty();
    Ok(body.split(|b| *b == 0).filter(move |_| !empty))
}

/// A path Git printed, as a [`RelPath`]: `None` when it is not UTF-8 or not a valid relative path
/// (too long; Git never prints `.` or `..` components or an absolute path).
pub fn rel_path(bytes: &[u8]) -> Option<RelPath> {
    core::str::from_utf8(bytes)
        .ok()
        .and_then(|text| RelPath::new(text).ok())
}

/// Paths collected from Git's output: each once, sorted, and those with no [`RelPath`] counted.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The representable ones.
    pub paths: Vec<RelPath>,
    /// How many were not.
    pub unrepresentable: u64,
}

impl Paths {
    /// Adds one path.
    pub fn push(&mut self, bytes: &[u8]) {
        match rel_path(bytes) {
            Some(path) => self.paths.push(path),
            None => self.unrepresentable = self.unrepresentable.saturating_add(1),
        }
    }

    /// Sorts and drops repeats (an unmerged path is listed once per stage).
    #[must_use]
    pub fn finish(mut self) -> Self {
        self.paths.sort_unstable();
        self.paths.dedup();
        self
    }
}

/// `diff --name-only -z`: one path per record.
pub fn names(bytes: &[u8]) -> Result<Paths, Malformed> {
    let mut paths = Paths::default();
    for record in records(bytes)? {
        if record.is_empty() {
            return Err(Malformed);
        }
        paths.push(record);
    }
    Ok(paths.finish())
}

/// `ls-files -z`, keeping `.java` files only (the pathspec already asked for them; a name that
/// only matched it some other way is dropped here, by its bytes, before it is decoded).
pub fn java_names(bytes: &[u8]) -> Result<Paths, Malformed> {
    let mut paths = Paths::default();
    for record in records(bytes)? {
        if record.is_empty() {
            return Err(Malformed);
        }
        let name = record.rsplit(|b| *b == b'/').next().unwrap_or(record);
        if name.len() > JAVA_EXTENSION.len() && name.ends_with(JAVA_EXTENSION.as_bytes()) {
            paths.push(record);
        }
    }
    Ok(paths.finish())
}

/// What `status -z --porcelain=v2` reported.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Status {
    /// Tracked paths that differ from `HEAD` in the index or the working tree, and unmerged ones.
    pub working: Paths,
    /// Untracked paths.
    pub untracked: Paths,
}

/// `status -z --porcelain=v2`. An untracked directory (`? dir/`, which `--untracked-files=all`
/// prints only for a nested repository or linked worktree) is left out: it is its own view, and
/// this one never indexes it (plan §4.2). Each record is a header (`# ...`), a changed entry
/// (`1 XY sub mH mI mW hH hI path`), a renamed or copied one (`2 ... Xscore path`, followed by
/// its original path as the next record), an unmerged one (`u XY sub m1 m2 m3 mW h1 h2 h3
/// path`), an untracked one (`? path`) or an ignored one (`! path`, not asked for). The path is
/// the rest of the record after a fixed number of fields, so it may hold spaces.
pub fn status(bytes: &[u8]) -> Result<Status, Malformed> {
    let mut status = Status::default();
    let mut records = records(bytes)?;
    while let Some(record) = records.next() {
        match record.split_first() {
            Some((b'#', _)) => {}
            Some((b'1', _)) => status.working.push(field_path(record, 8)?),
            Some((b'u', _)) => status.working.push(field_path(record, 10)?),
            Some((b'2', _)) => {
                status.working.push(field_path(record, 9)?);
                let original = records.next().ok_or(Malformed)?;
                if original.is_empty() {
                    return Err(Malformed);
                }
                status.working.push(original);
            }
            Some((b'?', rest)) => {
                let path = prefixed_path(rest)?;
                // A directory: a nested repository or linked worktree, its own view (plan §4.2).
                if !path.ends_with(b"/") {
                    status.untracked.push(path);
                }
            }
            Some((b'!', rest)) => {
                prefixed_path(rest)?;
            }
            _ => return Err(Malformed),
        }
    }
    Ok(Status {
        working: status.working.finish(),
        untracked: status.untracked.finish(),
    })
}

/// The path after `fields` space-separated fields (the entry's letter first), refused when there
/// are fewer, when the `XY` field is not two characters, or when the path is empty.
fn field_path(record: &[u8], fields: usize) -> Result<&[u8], Malformed> {
    let mut parts = record.splitn(fields.saturating_add(1), |b| *b == b' ');
    let mut seen = 0;
    for (index, part) in parts.by_ref().take(fields).enumerate() {
        if part.is_empty() || (index == 1 && part.len() != 2) {
            return Err(Malformed);
        }
        seen = index.saturating_add(1);
    }
    if seen != fields {
        return Err(Malformed);
    }
    match parts.next() {
        Some(path) if !path.is_empty() => Ok(path),
        _ => Err(Malformed),
    }
}

/// The path of a `? path` or `! path` record, after its letter.
fn prefixed_path(rest: &[u8]) -> Result<&[u8], Malformed> {
    match rest.split_first() {
        Some((b' ', path)) if !path.is_empty() => Ok(path),
        _ => Err(Malformed),
    }
}

/// The most filter drivers a repository may configure; more is refused, so the environment that
/// turns them off stays bounded.
pub const MAX_FILTER_DRIVERS: usize = 1024;

/// The driver names of `config -z --name-only --get-regexp '^filter\.'`: each key is
/// `filter.<driver>.<variable>`, and the driver is everything between the first and the last dot
/// (it may hold dots and `=`, kept as bytes). A key with no driver (`filter.<variable>`) names
/// none. Each driver once, sorted; more than [`MAX_FILTER_DRIVERS`] is refused.
pub fn filter_drivers(bytes: &[u8]) -> Result<Vec<Vec<u8>>, Malformed> {
    let mut drivers = Vec::new();
    for record in records(bytes)? {
        let rest = record.strip_prefix(b"filter.").ok_or(Malformed)?;
        let Some(dot) = rest.iter().rposition(|b| *b == b'.') else {
            continue;
        };
        let driver = rest.get(..dot).ok_or(Malformed)?;
        drivers.push(driver.to_vec());
    }
    drivers.sort_unstable();
    drivers.dedup();
    if drivers.len() > MAX_FILTER_DRIVERS {
        return Err(Malformed);
    }
    Ok(drivers)
}

/// One line of `rev-parse` output: an object id and a newline.
pub fn object_line(bytes: &[u8]) -> Result<ObjectId, Malformed> {
    let line = bytes.strip_suffix(b"\n").ok_or(Malformed)?;
    core::str::from_utf8(line)
        .ok()
        .and_then(ObjectId::parse)
        .ok_or(Malformed)
}

/// `rev-parse --show-toplevel --git-dir --git-common-dir` with `--path-format=absolute`: exactly
/// three non-empty absolute paths, each on its own line. A path holding a newline makes more
/// lines and is refused.
pub fn three_paths(bytes: &[u8]) -> Result<[&[u8]; 3], Malformed> {
    let body = bytes.strip_suffix(b"\n").ok_or(Malformed)?;
    let mut lines = body.split(|b| *b == b'\n');
    let mut next = || match lines.next() {
        Some(line) if line.first() == Some(&b'/') => Ok(line),
        _ => Err(Malformed),
    };
    let paths = [next()?, next()?, next()?];
    if lines.next().is_some() {
        return Err(Malformed);
    }
    Ok(paths)
}

/// The content of `HEAD` or of a loose ref: `ref: <name>` (a symbolic ref) or an object id, with
/// an optional trailing newline. A name must start with `refs/` and be a valid relative path (it
/// is joined to a Git directory, so it may never climb out of it).
pub fn head(bytes: &[u8]) -> HeadState {
    let line = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let Ok(text) = core::str::from_utf8(line) else {
        return HeadState::Unreadable;
    };
    if let Some(name) = text.strip_prefix("ref: ") {
        let name = name.trim_end_matches(' ');
        if name.starts_with("refs/") && RelPath::new(name).is_ok() {
            return HeadState::Symbolic(String::from(name));
        }
        return HeadState::Unreadable;
    }
    ObjectId::parse(text).map_or(HeadState::Unreadable, HeadState::Detached)
}

/// The path a `.git` file names (`gitdir: <path>`, an optional trailing newline), as Git reads it:
/// absolute, or relative to the directory holding the `.git` file. `None` for anything else.
pub fn gitfile(bytes: &[u8]) -> Option<&std::path::Path> {
    use std::os::unix::ffi::OsStrExt;
    let line = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let path = line.strip_prefix(b"gitdir: ")?;
    (!path.is_empty() && !path.contains(&0) && !path.contains(&b'\n'))
        .then(|| std::path::Path::new(std::ffi::OsStr::from_bytes(path)))
}

/// Where a loose ref lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefHome {
    /// `$GIT_DIR`: refs private to one worktree.
    GitDir,
    /// The common directory: branches, tags, remotes.
    CommonDir,
}

/// Git's per-worktree refs (`refs/bisect/`, `refs/worktree/`, `refs/rewritten/`) live in
/// `$GIT_DIR`; every other ref in the common directory (git-worktree(1), "Refs").
#[must_use]
pub fn ref_home(name: &str) -> RefHome {
    const PER_WORKTREE: [&str; 3] = ["refs/bisect/", "refs/worktree/", "refs/rewritten/"];
    if PER_WORKTREE.iter().any(|prefix| name.starts_with(prefix)) {
        RefHome::GitDir
    } else {
        RefHome::CommonDir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(paths: &Paths) -> Vec<&str> {
        paths.paths.iter().map(RelPath::as_str).collect()
    }

    #[test]
    fn should_split_records_when_output_is_nul_terminated() {
        // given
        let output = b"a b\0c\0";

        // when
        let parsed: Vec<&[u8]> = records(output).unwrap().collect();
        let empty: Vec<&[u8]> = records(b"").unwrap().collect();
        let cut = records(b"a\0b").err();

        // then
        assert_eq!(parsed, [&b"a b"[..], b"c"]);
        assert!(empty.is_empty());
        assert_eq!(cut, Some(Malformed));
    }

    #[test]
    fn should_count_a_path_when_it_is_not_utf8() {
        // given
        let output = b"src/A.java\0src/\xff.java\0src/A.java\0";

        // when
        let paths = names(output).unwrap();

        // then
        assert_eq!(strs(&paths), ["src/A.java"]);
        assert_eq!(paths.unrepresentable, 1);
    }

    #[test]
    fn should_keep_java_files_only_when_listing_tracked_files() {
        // given
        let output = b"a/B.java\0a/B.javax\0.java\0x/.java\0README\0c d/E.java\0x/\xfe.java\0";

        // when
        let paths = java_names(output).unwrap();

        // then
        assert_eq!(strs(&paths), ["a/B.java", "c d/E.java"]);
        assert_eq!(paths.unrepresentable, 1);
    }

    #[test]
    fn should_read_every_entry_kind_when_parsing_status_v2() {
        // given
        let output = [
            &b"# branch.oid abc\0"[..],
            b"1 .M N... 100644 100644 100644 0000 0000 src/with space.java\0",
            b"1 D. N... 100644 000000 000000 0000 0000 gone.java\0",
            b"2 R. N... 100644 100644 100644 0000 0000 R100 new.java\0old.java\0",
            b"u UU N... 100644 100644 100644 100644 0000 0000 0000 both.java\0",
            b"? new dir/U.java\0",
            b"? .claude/worktrees/agent/\0",
            b"! ignored.class\0",
        ]
        .concat();

        // when
        let status = status(&output).unwrap();

        // then
        assert_eq!(
            strs(&status.working),
            [
                "both.java",
                "gone.java",
                "new.java",
                "old.java",
                "src/with space.java"
            ]
        );
        assert_eq!(strs(&status.untracked), ["new dir/U.java"]);
    }

    #[test]
    fn should_refuse_status_when_an_entry_is_short_or_unknown() {
        // given
        let cases: [&[u8]; 7] = [
            b"1 .M N... 100644\0",
            b"1 .M N... 100644 100644 100644 0000 0000 \0",
            b"1 .MM N... 100644 100644 100644 0000 0000 a\0",
            b"2 R. N... 100644 100644 100644 0000 0000 R100 new.java\0",
            b"?\0",
            b"x what\0",
            b"? a",
        ];

        // when
        let results: Vec<_> = cases.iter().map(|case| status(case).err()).collect();

        // then
        assert!(results.iter().all(|r| *r == Some(Malformed)), "{results:?}");
    }

    #[test]
    fn should_name_each_filter_driver_once_when_its_keys_are_listed() {
        // given
        let output = b"filter.lfs.clean\0filter.lfs.process\0filter.a.b=c.smudge\0filter.required\0filter..clean\0";

        // when
        let drivers = filter_drivers(output);
        let refused = filter_drivers(b"core.fsmonitor\0");

        // then
        assert_eq!(
            drivers,
            Ok(vec![b"".to_vec(), b"a.b=c".to_vec(), b"lfs".to_vec()])
        );
        assert_eq!(refused, Err(Malformed));
    }

    #[test]
    fn should_read_an_object_id_when_rev_parse_prints_one_line() {
        // given
        let line = b"0123456789abcdef0123456789abcdef01234567\n";

        // when
        let id = object_line(line);
        let refused = [
            &b"0123456789abcdef0123456789abcdef01234567"[..],
            b"\n",
            b"HEAD\n",
        ]
        .map(object_line);

        // then
        assert_eq!(
            id.map(|id| id.as_str().to_owned()),
            Ok("0123456789abcdef0123456789abcdef01234567".to_owned())
        );
        assert!(refused.iter().all(Result::is_err));
    }

    #[test]
    fn should_take_three_absolute_paths_when_confirming_a_root() {
        // given
        let ok = b"/r\n/r/.git\n/r/.git\n";

        // when
        let parsed = three_paths(ok);
        let refused = [
            &b"/r\n/r/.git\n"[..],
            b"/r\n/r/.git\n/r/.git\n/x\n",
            b"/r\n.git\n/r/.git\n",
            b"/r\n/r/.git\n/r/.git",
            b"",
        ]
        .map(three_paths);

        // then
        assert_eq!(parsed, Ok([&b"/r"[..], b"/r/.git", b"/r/.git"]));
        assert!(refused.iter().all(Result::is_err), "{refused:?}");
    }

    #[test]
    fn should_read_head_when_it_is_a_symbolic_ref_or_an_id() {
        // given
        let id = "0123456789abcdef0123456789abcdef01234567";

        // when
        let symbolic = head(b"ref: refs/heads/main\n");
        let no_newline = head(b"ref: refs/heads/feature/x");
        let detached = head(format!("{id}\n").as_bytes());
        let refused = [
            &b"ref: refs/../../etc/passwd\n"[..],
            b"ref: HEAD\n",
            b"ref: /abs\n",
            b"ref: refs/heads/a\0b\n",
            b"garbage\n",
            b"\xff\n",
            b"",
        ]
        .map(head);

        // then
        assert_eq!(symbolic, HeadState::Symbolic("refs/heads/main".to_owned()));
        assert_eq!(
            no_newline,
            HeadState::Symbolic("refs/heads/feature/x".to_owned())
        );
        assert_eq!(detached, HeadState::Detached(ObjectId::parse(id).unwrap()));
        assert!(
            refused.iter().all(|h| *h == HeadState::Unreadable),
            "{refused:?}"
        );
    }

    #[test]
    fn should_read_the_gitdir_when_a_git_file_names_one() {
        // given
        let cases: [&[u8]; 6] = [
            b"gitdir: /r/.git/worktrees/a\n",
            b"gitdir: ../.git/modules/m",
            b"gitdir: \n",
            b"gitdir: /a\n/b\n",
            b"other: /x\n",
            b"",
        ];

        // when
        let parsed: Vec<Option<&str>> = cases
            .iter()
            .map(|c| gitfile(c).map(|p| p.to_str().unwrap()))
            .collect();

        // then
        assert_eq!(
            parsed,
            [
                Some("/r/.git/worktrees/a"),
                Some("../.git/modules/m"),
                None,
                None,
                None,
                None
            ]
        );
    }

    #[test]
    fn should_keep_per_worktree_refs_in_the_git_dir_when_locating_a_ref() {
        // given
        let names = [
            "refs/heads/main",
            "refs/bisect/bad",
            "refs/worktree/x",
            "refs/rewritten/y",
            "refs/tags/t",
        ];

        // when
        let homes = names.map(ref_home);

        // then
        assert_eq!(
            homes,
            [
                RefHome::CommonDir,
                RefHome::GitDir,
                RefHome::GitDir,
                RefHome::GitDir,
                RefHome::CommonDir
            ]
        );
    }
}
