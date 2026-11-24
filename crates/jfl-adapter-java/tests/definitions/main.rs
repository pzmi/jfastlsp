//! The syntax-tree resolver against javac (IDX-10): definitions answered by the queries over this
//! crate's indexer (its summaries, and its parse of the request's file for the scope facts), at
//! every reference site of the fixture project `fixtures/java/projects/shop` (both reviews'
//! probes among them) and of a corpus of nested constructs generated from a fixed seed
//! (`generator.rs` beside this file), checked against where javac binds each site.
//!
//! **The oracle.** `just javac-bindings write` compiles each project with the pinned JDK's javac
//! (`tools/javac-bindings/JavacBindings.java`, the Compiler Tree API: attribution only) and
//! records every binding in `tests/fixtures/javac/<project>.tsv`, with each file's FNV-1a digest;
//! `just javac-bindings check` recompiles and fails on any difference, which is the step a JDK
//! stage of the gate runs (EVAL-07's `java-fixtures`). These tests run without a JDK: they read
//! the recording, and fail when a file it was made from has changed since.
//!
//! **The property.** Every answer that is `exact` names the declaration javac binds the site
//! to; an answer javac cannot confirm is never `exact`. And the proof is used: the corpus has
//! enough provable sites that a resolver which never proves anything fails too.

#[cfg(test)]
mod generator;
#[cfg(test)]
mod references;

#[cfg(test)]
mod given {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use jfl_adapter_java::{Grammar, GrammarSpec, JavaIndexer};
    use jfl_application::queries::{definition, references};
    use jfl_application::syntax_indexer::SyntaxIndexer;
    use jfl_application::{
        Answer, ClockDeadline, CountingClock, Pinned, QueryCache, QueryConfig, ReferencesQuery,
        ServiceConfig, ViewService,
    };
    use jfl_domain::{
        ByteOffset, DeclKind, Envelope, FileSummary, FileUri, Freshness, LineIndex, Position,
        PositionEncoding, UnixMillis,
    };
    use jfl_testkit::exec::{InlinePool, block_on};
    use jfl_testkit::fakes::clock::FakeWallClock;
    use jfl_testkit::fakes::hasher::FnvHasher;
    use jfl_testkit::fakes::repo_probe::FakeRepoProbe;
    use jfl_testkit::fakes::source_tree::FakeSourceTree;
    use jfl_testkit::fakes::summary_store::{Backing, MemorySummaryStore};
    use jfl_testkit::golden;

    pub(crate) const ROOT: &str = "/w/m";
    pub(crate) const UTF16: PositionEncoding = PositionEncoding::Utf16;

    pub(crate) type Service<X = JavaIndexer<InlinePool>> =
        ViewService<FakeSourceTree, FakeRepoProbe, X, MemorySummaryStore, FakeWallClock, FnvHasher>;

    pub(crate) fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/javac")
    }

    pub(crate) fn uri(path: &str) -> FileUri {
        FileUri::from_path(path).unwrap()
    }

    pub(crate) fn indexer() -> JavaIndexer<InlinePool> {
        JavaIndexer::new(Grammar::new(&GrammarSpec::orchard()).unwrap(), InlinePool)
    }

    /// A project's files, by path under the project, and the view service over a copy of them.
    pub(crate) struct World<X: SyntaxIndexer = JavaIndexer<InlinePool>> {
        files: Vec<(String, Vec<u8>)>,
        service: Service<X>,
    }

    /// The fixture project `shop`'s files.
    pub(crate) fn shop_files() -> Vec<(String, Vec<u8>)> {
        golden::corpus("projects/shop")
            .unwrap()
            .into_iter()
            .map(|f| {
                (
                    f.path.strip_prefix("projects/shop/").unwrap().to_owned(),
                    f.bytes,
                )
            })
            .collect()
    }

    /// The generated corpus's files, as committed.
    pub(crate) fn generated_files() -> Vec<(String, Vec<u8>)> {
        golden::files_under(&fixtures().join("generated"), ".java").unwrap()
    }

    impl World {
        pub(crate) fn new(files: Vec<(String, Vec<u8>)>) -> Self {
            World::with_indexer(files, indexer())
        }

        /// The fixture project `shop`.
        pub(crate) fn shop() -> Self {
            Self::new(shop_files())
        }

        /// The generated corpus as committed.
        pub(crate) fn generated() -> Self {
            Self::new(generated_files())
        }
    }

    impl<X: SyntaxIndexer> World<X> {
        /// The view service over `files`, indexed by `x`.
        pub(crate) fn with_indexer(files: Vec<(String, Vec<u8>)>, x: X) -> Self {
            let tree = FakeSourceTree::new(uri("/w"), 1024);
            let probe = FakeRepoProbe::new();
            for (path, bytes) in &files {
                tree.write(&format!("m/{path}"), bytes).unwrap();
            }
            let committed: Vec<(&str, &[u8])> = files
                .iter()
                .map(|(p, b)| (p.as_str(), b.as_slice()))
                .collect();
            probe.init(&uri(ROOT), &committed).unwrap();
            let store = MemorySummaryStore::open(
                Backing::new(),
                1 << 30,
                Arc::new(CountingClock::starting_at(1)),
            );
            let service = ViewService::new(
                (tree, probe, x, store),
                FakeWallClock::at(1_800_000_000),
                FnvHasher::default(),
                ServiceConfig::new("t1"),
            );
            Self { files, service }
        }

        pub(crate) fn files(&self) -> &[(String, Vec<u8>)] {
            &self.files
        }

        pub(crate) fn text(&self, path: &str) -> &str {
            let (_, bytes) = self.files.iter().find(|(p, _)| p == path).unwrap();
            core::str::from_utf8(bytes).unwrap()
        }

        pub(crate) fn summary(&self, path: &str) -> FileSummary {
            indexer()
                .index_blocking(self.text(path).as_bytes())
                .unwrap()
                .summary
        }

        pub(crate) fn ask(&mut self, path: &str) -> Answer {
            block_on(self.service.request(&uri(&format!("{ROOT}/{path}")))).unwrap()
        }

        /// The references at byte `offset` of `path`.
        pub(crate) fn refs(&mut self, path: &str, offset: u32, cache: &QueryCache) -> Envelope {
            let position = position(self.text(path), offset);
            let answer = self.ask(path);
            let pinned = Pinned {
                answer: &answer,
                head: None,
                freshness: Freshness {
                    synced_at: Some(UnixMillis(1_800_000_000_000)),
                    pending_events: 0,
                },
            };
            let clock = FakeWallClock::at(1_800_000_000);
            let config = QueryConfig::default();
            let deadline = ClockDeadline::new(&clock, config.references_budget);
            block_on(references(
                &pinned,
                &self.service.sources(),
                ReferencesQuery {
                    position,
                    ..ReferencesQuery::default()
                },
                UTF16,
                &config,
                &deadline,
                cache,
            ))
            .unwrap()
        }

        /// The definition at byte `offset` of `path`.
        pub(crate) fn def(&mut self, path: &str, offset: u32, cache: &QueryCache) -> Envelope {
            let position = position(self.text(path), offset);
            let answer = self.ask(path);
            let pinned = Pinned {
                answer: &answer,
                head: None,
                freshness: Freshness {
                    synced_at: Some(UnixMillis(1_800_000_000_000)),
                    pending_events: 0,
                },
            };
            let clock = FakeWallClock::at(1_800_000_000);
            let config = QueryConfig::default();
            let deadline = ClockDeadline::new(&clock, config.definition_budget);
            block_on(definition(
                &pinned,
                &self.service.sources(),
                position,
                UTF16,
                &config,
                &deadline,
                cache,
            ))
            .unwrap()
        }
    }

    pub(crate) fn position(text: &str, offset: u32) -> Position {
        LineIndex::new(text)
            .unwrap()
            .position(ByteOffset::new(offset), UTF16)
            .unwrap()
    }

    /// FNV-1a, 64 bits: the digest `JavacBindings.java` writes beside each file.
    pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, &b| {
            (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    /// Where javac binds a site.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Binding {
        /// A declaration of the project: its kind, simple name, file and the bytes of its tree.
        Decl {
            kind: String,
            name: String,
            path: String,
            start: u32,
            end: u32,
        },
        /// Anything else: a declaration javac read from elsewhere, a local, a type variable, a
        /// package (the line as recorded).
        Other(String),
    }

    /// A project's recording: each file's digest, and each site's binding by `(path, start, end)`.
    pub(crate) struct Recording {
        digests: Vec<(String, u64)>,
        pub(crate) sites: std::collections::BTreeMap<(String, u32, u32), Binding>,
    }

    pub(crate) fn recording(project: &str) -> Recording {
        let path = fixtures().join(format!("{project}.tsv"));
        let text = golden::files_under(&fixtures(), &format!("{project}.tsv"))
            .unwrap()
            .into_iter()
            .find(|(p, _)| *p == format!("{project}.tsv"))
            .map(|(_, b)| String::from_utf8(b).unwrap())
            .unwrap_or_else(|| {
                panic!(
                    "{}: missing; run `just javac-bindings write`",
                    path.display()
                )
            });
        let mut digests = Vec::new();
        let mut sites = std::collections::BTreeMap::new();
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let fields: Vec<&str> = line.split('\t').collect();
            match fields.as_slice() {
                ["file", path, digest] => {
                    digests.push(((*path).to_owned(), u64::from_str_radix(digest, 16).unwrap()));
                }
                [
                    "site",
                    path,
                    start,
                    end,
                    "decl",
                    kind,
                    name,
                    dpath,
                    dstart,
                    dend,
                ] => {
                    sites.insert(
                        (
                            (*path).to_owned(),
                            start.parse().unwrap(),
                            end.parse().unwrap(),
                        ),
                        Binding::Decl {
                            kind: (*kind).to_owned(),
                            name: (*name).to_owned(),
                            path: (*dpath).to_owned(),
                            start: dstart.parse().unwrap(),
                            end: dend.parse().unwrap(),
                        },
                    );
                }
                ["site", path, start, end, rest @ ..] => {
                    sites.insert(
                        (
                            (*path).to_owned(),
                            start.parse().unwrap(),
                            end.parse().unwrap(),
                        ),
                        Binding::Other(rest.join(" ")),
                    );
                }
                other => panic!("{project}.tsv: an unknown line {other:?}"),
            }
        }
        Recording { digests, sites }
    }

    /// The recording is of exactly these files' bytes: a changed fixture needs a new recording.
    pub(crate) fn assert_fresh<X: SyntaxIndexer>(
        world: &World<X>,
        recorded: &Recording,
        project: &str,
    ) {
        let now: Vec<(String, u64)> = world
            .files
            .iter()
            .map(|(p, b)| (p.clone(), fnv1a64(b)))
            .collect();
        let mut want = recorded.digests.clone();
        want.sort();
        assert_eq!(
            now, want,
            "{project}.tsv was recorded from other files: run `just javac-bindings write`"
        );
    }

    /// The declaration of `summary` a javac binding names: the first one (by name) of that name
    /// whose name lies in the bound tree's bytes (a constructor by its kind, javac calls it
    /// `<init>`).
    pub(crate) fn declared(
        summary: &FileSummary,
        kind: &str,
        name: &str,
        start: u32,
        end: u32,
    ) -> Option<u32> {
        summary
            .declarations()
            .filter(|(_, d)| {
                let inside = start <= d.name_range.start().get() && d.name_range.end().get() <= end;
                let named = if kind == "CONSTRUCTOR" {
                    d.kind == DeclKind::Constructor
                } else {
                    summary.str(d.name) == name
                };
                inside && named
            })
            .map(|(_, d)| d.name_range.start().get())
            .min()
    }

    /// What the differential found over one project.
    #[derive(Debug, Default)]
    pub(crate) struct Tally {
        pub(crate) sites: usize,
        pub(crate) exact: usize,
        pub(crate) wrong: Vec<String>,
    }

    /// Asks the definition at every reference site of `world` and checks each `exact` answer
    /// against `recorded`.
    pub(crate) fn differential(world: &mut World, recorded: &Recording) -> Tally {
        let cache = QueryCache::new();
        let mut tally = Tally::default();
        let paths: Vec<String> = world.files.iter().map(|(p, _)| p.clone()).collect();
        for path in &paths {
            let summary = world.summary(path);
            for site in summary.references() {
                let (start, end) = (site.range.start().get(), site.range.end().get());
                // Inside the name, so no neighbouring site touches the offset.
                let offset = if end > start.saturating_add(1) {
                    start.saturating_add(1)
                } else {
                    start
                };
                let envelope = world.def(path, offset, &cache);
                tally.sites = tally.sites.saturating_add(1);
                let exact = envelope.exact();
                if exact.is_empty() {
                    continue;
                }
                tally.exact = tally.exact.saturating_add(1);
                let here = format!("{path}:{start}-{end} `{}`", summary.str(site.name));
                let Some(Binding::Decl {
                    kind,
                    name,
                    path: dpath,
                    start: dstart,
                    end: dend,
                }) = recorded.sites.get(&(path.clone(), start, end))
                else {
                    tally.wrong.push(format!(
                        "{here}: exact, but javac binds {:?}",
                        recorded.sites.get(&(path.clone(), start, end))
                    ));
                    continue;
                };
                let target = world.summary(dpath);
                let want = declared(&target, kind, name, *dstart, *dend)
                    .map(|at| (format!("{ROOT}/{dpath}"), position(world.text(dpath), at)));
                let got: Vec<(String, Position)> = exact
                    .iter()
                    .map(|l| (l.uri.path().to_owned(), l.range.start()))
                    .collect();
                if want.as_ref().is_none_or(|w| got != [w.clone()]) {
                    tally.wrong.push(format!(
                        "{here}: exact on {got:?}, javac binds {kind} {name} of {dpath} at {want:?}"
                    ));
                }
            }
        }
        tally
    }
}

use given::*;
use jfl_testkit::golden;

#[test]
fn should_regenerate_the_committed_corpus_when_the_seed_is_kept() {
    // given
    let files = generator::generate(generator::SEED);
    // when
    let failures: Vec<String> = files
        .iter()
        .filter_map(|(path, text)| {
            golden::check_file(&fixtures().join("generated").join(path), text).err()
        })
        .collect();
    let committed = golden::files_under(&fixtures().join("generated"), ".java").unwrap();
    // then
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        committed.len(),
        files.len(),
        "the generated corpus holds files the generator does not make"
    );
}

#[test]
fn should_match_javac_or_stay_possible_when_any_site_of_shop_is_asked() {
    // given
    let mut world = World::shop();
    let recorded = recording("shop");
    assert_fresh(&world, &recorded, "shop");
    // when
    let tally = differential(&mut world, &recorded);
    // then
    assert!(
        tally.wrong.is_empty(),
        "{} of {} exact answers differ from javac:\n{}",
        tally.wrong.len(),
        tally.exact,
        tally.wrong.join("\n")
    );
    assert!(
        tally.sites >= 203,
        "the project shrank: {} sites",
        tally.sites
    );
    assert!(
        tally.exact >= SHOP_EXACT,
        "{} of {} sites exact, fewer than the {SHOP_EXACT} the resolver proved when it landed",
        tally.exact,
        tally.sites
    );
}

#[test]
fn should_match_javac_or_stay_possible_when_any_site_of_the_generated_corpus_is_asked() {
    // given
    let mut world = World::generated();
    let recorded = recording("generated");
    assert_fresh(&world, &recorded, "generated");
    // when
    let tally = differential(&mut world, &recorded);
    // then
    assert!(
        tally.wrong.is_empty(),
        "{} of {} exact answers differ from javac:\n{}",
        tally.wrong.len(),
        tally.exact,
        tally.wrong.join("\n")
    );
    assert!(
        tally.exact >= GENERATED_EXACT,
        "{} of {} sites exact, fewer than the {GENERATED_EXACT} the resolver proved when it landed",
        tally.exact,
        tally.sites
    );
}

/// How many sites of `shop` were `exact` when the resolver landed: fewer means a rule stopped
/// proving what it proved (a floor, never lowered to get green).
const SHOP_EXACT: usize = 49;

/// The same floor for the generated corpus.
const GENERATED_EXACT: usize = 232;
