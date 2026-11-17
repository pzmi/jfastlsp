//! The E1 pipeline on a synthetic Java tree: counts, error detection, stage order and
//! determinism across thread counts (nothing from the real corpus is used or needed).

use std::error::Error;
use std::path::{Path, PathBuf};

use jfl_bench::e1::{self, Options, Stage};

struct Tree(PathBuf);

impl Tree {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("jfl-bench-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("p"))?;
        std::fs::create_dir_all(dir.join(".hidden"))?;
        std::fs::write(
            dir.join("p/A.java"),
            "package p;\nimport java.util.List;\npublic class A { int x; void m() {} A() {} }\n",
        )?;
        std::fs::write(
            dir.join("p/B.java"),
            "package p;\ninterface B { void f(); }\n",
        )?;
        std::fs::write(dir.join("p/Broken.java"), "package p;\nclass { int ; \n")?;
        std::fs::write(dir.join(".hidden/H.java"), "class H {}\n")?;
        std::fs::write(dir.join("p/notes.txt"), "class NotJava {}\n")?;
        Ok(Self(dir))
    }

    fn root(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(root: &Path, threads: usize, upto: Stage) -> Result<e1::RunResult, Box<dyn Error>> {
    Ok(e1::run(&Options {
        root: root.to_path_buf(),
        threads,
        upto,
    })?)
}

#[test]
fn should_count_java_files_only_when_hidden_directories_and_other_files_exist() {
    // given
    let tree = Tree::new("count").unwrap();
    // when
    let r = run(tree.root(), 2, Stage::Extract).unwrap();
    // then: A, B and Broken; not .hidden/H.java, not notes.txt
    assert_eq!(r.files, 3);
    assert_eq!(r.failed_files, 0);
}

#[test]
fn should_extract_package_imports_and_declarations_when_the_file_is_valid() {
    // given
    let tree = Tree::new("extract").unwrap();
    // when
    let r = run(tree.root(), 1, Stage::Extract).unwrap();
    // then: A: package, import, class, field, method, constructor (6); B: package, interface,
    // method (3); Broken adds what error recovery salvages, so the total is at least 9
    assert!(r.decls >= 9, "decls = {}", r.decls);
}

#[test]
fn should_flag_the_file_with_a_syntax_error_when_parsing() {
    // given
    let tree = Tree::new("errors").unwrap();
    // when
    let r = run(tree.root(), 1, Stage::Parse).unwrap();
    // then
    assert_eq!(r.err_files, 1);
}

#[test]
fn should_do_only_the_stages_asked_for_when_the_pipeline_stops_early() {
    // given
    let tree = Tree::new("stages").unwrap();
    // when
    let read = run(tree.root(), 1, Stage::Read).unwrap();
    let parse = run(tree.root(), 1, Stage::Parse).unwrap();
    // then
    assert_eq!(read.decls, 0);
    assert_eq!(read.err_files, 0);
    assert_eq!(parse.decls, 0);
    assert_eq!(parse.err_files, 1);
    assert_eq!(read.files, parse.files);
}

#[test]
fn should_extract_the_same_thing_when_the_thread_count_differs() {
    // given
    let tree = Tree::new("threads").unwrap();
    // when
    let one = run(tree.root(), 1, Stage::Extract).unwrap();
    let four = run(tree.root(), 4, Stage::Extract).unwrap();
    // then
    assert_eq!(
        (one.files, one.decls, one.checksum),
        (four.files, four.decls, four.checksum)
    );
    assert_ne!(one.checksum, 0);
}

#[test]
fn should_name_the_stages_when_a_stage_is_parsed_from_text() {
    // given
    // when
    // then
    for s in [Stage::Read, Stage::Hash, Stage::Parse, Stage::Extract] {
        assert_eq!(Stage::from_name(s.as_str()).unwrap(), s);
    }
    assert!(Stage::from_name("index").is_err());
    assert!(Stage::Read < Stage::Hash && Stage::Parse < Stage::Extract);
}

#[test]
fn should_count_a_file_over_the_cap_as_failed_and_not_read_it_when_it_is_too_big() {
    // given: a sparse file one byte over the cap
    let tree = Tree::new("cap").unwrap();
    let big = std::fs::File::create(tree.root().join("p/Huge.java")).unwrap();
    big.set_len(e1::MAX_FILE_BYTES + 1).unwrap();
    // when
    let r = run(tree.root(), 1, Stage::Read).unwrap();
    // then
    assert_eq!(r.files, 4);
    assert_eq!(r.failed_files, 1);
}
