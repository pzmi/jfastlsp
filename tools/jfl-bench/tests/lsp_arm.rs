//! The comparison arms' inputs on the synthetic multi-module fixture: usage sites across module
//! boundaries and the source roots given to jdtls. No jdtls, kmp-lsp or corpus is needed; the
//! arms themselves run through `jfl-bench run` (doc/bench/README.md).

use std::path::{Path, PathBuf};

use jfl_bench::arms;
use jfl_bench::queries;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/multi-module")
}

#[test]
fn should_query_a_use_in_another_file_when_the_picked_type_is_used_elsewhere() {
    // given
    let root = fixture();
    let picked = queries::pick(&root, 10, 1).unwrap();
    // when
    let sites = arms::usage_sites(&root, &picked);
    // then: one site per pick; a use is in another file, at the name, and Main uses Money
    assert_eq!(sites.len(), picked.len());
    assert_eq!(picked.len(), 10);
    for (site, q) in sites.iter().zip(&picked) {
        let text = std::fs::read_to_string(&site.file).unwrap();
        let line = text.lines().nth(site.line as usize).unwrap();
        let at: String = line.chars().skip(site.character as usize).collect();
        assert!(at.starts_with(&q.name), "{q:?} -> {site:?}: {line}");
        if !site.at_declaration {
            assert_ne!(site.file, root.join(&q.file));
        }
    }
    assert!(sites.iter().filter(|s| !s.at_declaration).count() >= 8);
}

#[test]
fn should_give_every_module_source_root_when_jdtls_imports_nothing() {
    // given
    let root = fixture();
    // when
    let roots = arms::source_paths(&root);
    // then
    assert_eq!(
        roots,
        [
            "ledger-api/src/main/java",
            "ledger-app/src/main/java",
            "ledger-core/src/main/java",
        ]
    );
}
