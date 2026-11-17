//! Seeded query picking on a synthetic tree: the same seed gives the same queries, names are
//! distinct, and a file with no type declaration is skipped.

use std::error::Error;
use std::path::{Path, PathBuf};

use jfl_bench::queries;

const FROZEN_SEED_1: [&str; 6] = ["Type0", "Type3", "Type1", "Type24", "Type16", "Type7"];

struct Tree(PathBuf);

impl Tree {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("jfl-bench-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src"))?;
        for i in 0..30 {
            std::fs::write(
                dir.join(format!("src/T{i}.java")),
                format!("package s;\npublic class Type{i} {{}}\n"),
            )?;
        }
        std::fs::write(
            dir.join("src/package-info.java"),
            "/** docs */\npackage s;\n",
        )?;
        std::fs::write(dir.join("src/Dup.java"), "class Type3 {}\n")?;
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

#[test]
fn should_pick_the_same_queries_when_the_seed_is_the_same() {
    // given
    let tree = Tree::new("seed-same").unwrap();
    // when
    let a = queries::pick(tree.root(), 10, 7).unwrap();
    let b = queries::pick(tree.root(), 10, 7).unwrap();
    // then
    assert_eq!(a, b);
    assert_eq!(a.len(), 10);
}

#[test]
fn should_pick_other_queries_when_the_seed_differs() {
    // given
    let tree = Tree::new("seed-differs").unwrap();
    // when
    let a = queries::pick(tree.root(), 10, 1).unwrap();
    let b = queries::pick(tree.root(), 10, 2).unwrap();
    // then
    assert_ne!(a, b);
}

#[test]
fn should_pick_distinct_names_and_skip_files_without_a_type_when_picking_everything() {
    // given
    let tree = Tree::new("distinct").unwrap();
    // when
    let all = queries::pick(tree.root(), 1000, 3).unwrap();
    // then: Type0..Type29 once each; package-info.java contributes nothing; Dup.java's Type3 is a duplicate
    let mut names: Vec<&str> = all.iter().map(|q| q.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), all.len());
    assert_eq!(all.len(), 30);
    assert!(
        all.iter()
            .all(|q| q.line == 1 && q.column == 13 || q.line == 0)
    );
}

#[test]
fn should_keep_the_slot_to_name_list_stable_when_the_seed_and_the_tree_are_fixed() {
    // given: the frozen slot order for seed 1 over the synthetic tree (a change here renames every
    // per-query series of the history: bump the series version deliberately, never by accident)
    let tree = Tree::new("frozen").unwrap();
    // when
    let names: Vec<String> = queries::pick(tree.root(), 6, 1)
        .unwrap()
        .into_iter()
        .map(|q| q.name)
        .collect();
    // then
    assert_eq!(names, FROZEN_SEED_1);
}
