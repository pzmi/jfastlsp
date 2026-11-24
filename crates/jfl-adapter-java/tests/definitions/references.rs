//! References against javac, and the completeness oracle (IDX-11; plan §2.1, ver §7.2): the
//! references query answered over this crate's indexer (its summaries for the scan, its parse for
//! the proofs), for every declaration of the fixture project `shop` and of the generated corpus.
//!
//! **The superset.** Every site javac binds to a declaration (the recording of
//! `tools/javac-bindings`, member references included) is in `exact ∪ possible` of that
//! declaration's references, whatever the answer says of its completeness; and every `exact` site
//! is one javac binds to it. Asked at a use too: javac's sites of the declaration it binds the use
//! to are in the answer, whether or not the use's binding was proved.
//!
//! **The completeness oracle** (ver §7.2). A seeded trial picks a declaration of a project and
//! adds one more use of it in a file that may use it (its own file for a private member, its
//! package for a package-private one, any file otherwise), indexes the project again, and asks for
//! the declaration's references: the new use is in `exact ∪ possible`, or the answer says
//! `complete=false`. A planted defect (an indexer that loses the sites of the edited file) makes it
//! fail, so the oracle can see a wrong `complete=true`.

use std::collections::{BTreeMap, BTreeSet};

use jfl_adapter_java::JavaIndexer;
use jfl_application::QueryCache;
use jfl_application::syntax_indexer::{Indexed, SyntaxIndexError, SyntaxIndexer};
use jfl_domain::{DeclKind, Envelope, FileSummary, Modifiers, Position};
use jfl_testkit::exec::InlinePool;

use super::generator::Rng;
use super::given::{
    Binding, ROOT, Recording, World, assert_fresh, declared, generated_files, indexer, position,
    recording, shop_files,
};

/// A place as the differential compares it: the file under the project, and the range.
type Place = (String, Position, Position);

/// The sites javac binds to each declaration of `world`'s summaries, by the declaration's file
/// and name start.
fn bound_sites<X: SyntaxIndexer>(
    world: &World<X>,
    recorded: &Recording,
) -> BTreeMap<(String, u32), Vec<Place>> {
    let mut summaries: BTreeMap<String, FileSummary> = BTreeMap::new();
    let mut out: BTreeMap<(String, u32), Vec<Place>> = BTreeMap::new();
    for ((path, start, end), binding) in &recorded.sites {
        let Binding::Decl {
            kind,
            name,
            path: dpath,
            start: dstart,
            end: dend,
        } = binding
        else {
            continue;
        };
        let summary = summaries
            .entry(dpath.clone())
            .or_insert_with(|| world.summary(dpath));
        let Some(at) = declared(summary, kind, name, *dstart, *dend) else {
            continue;
        };
        let text = world.text(path);
        out.entry((dpath.clone(), at)).or_default().push((
            path.clone(),
            position(text, *start),
            position(text, *end),
        ));
    }
    out
}

/// The places of `locations`, by file under the project.
fn places<'a>(locations: impl Iterator<Item = &'a jfl_domain::Location>) -> BTreeSet<Place> {
    locations
        .map(|l| {
            (
                l.uri
                    .path()
                    .strip_prefix(&format!("{ROOT}/"))
                    .unwrap_or(l.uri.path())
                    .to_owned(),
                l.range.start(),
                l.range.end(),
            )
        })
        .collect()
}

/// An offset inside a name, so no neighbouring site touches it.
fn inside(start: u32, end: u32) -> u32 {
    if end > start.saturating_add(1) {
        start.saturating_add(1)
    } else {
        start
    }
}

/// What the references differential found over one project.
#[derive(Debug, Default)]
struct Tally {
    /// Declarations asked.
    targets: usize,
    /// Sites javac binds to them.
    javac: usize,
    /// Sites answered `exact`.
    exact: usize,
    /// Answers that claimed `complete`.
    complete: usize,
    /// javac's sites missing from `exact ∪ possible`.
    missing: Vec<String>,
    /// `exact` sites javac does not bind to the declaration.
    wrong: Vec<String>,
}

impl Tally {
    /// Checks one answer against `want`, javac's sites of the declaration it was asked about.
    fn check(&mut self, here: &str, envelope: &Envelope, want: &[Place]) {
        let all = places(
            envelope
                .exact()
                .iter()
                .chain(envelope.possible().iter().map(|p| &p.location)),
        );
        let exact = places(envelope.exact().iter());
        let want_set: BTreeSet<Place> = want.iter().cloned().collect();
        self.exact = self.exact.saturating_add(exact.len());
        self.complete = self
            .complete
            .saturating_add(usize::from(envelope.complete()));
        for w in &want_set {
            if !all.contains(w) {
                self.missing.push(format!(
                    "{here}: javac's {w:?} is in neither exact nor possible (complete={})",
                    envelope.complete()
                ));
            }
        }
        for e in exact.difference(&want_set) {
            self.wrong
                .push(format!("{here}: exact {e:?} is not javac's"));
        }
    }
}

/// Asks the references of every declaration of `world` and checks each answer against javac.
fn differential(world: &mut World, recorded: &Recording) -> Tally {
    let bound = bound_sites(world, recorded);
    let cache = QueryCache::new();
    let mut tally = Tally::default();
    let paths: Vec<String> = world.files().iter().map(|(p, _)| p.clone()).collect();
    for path in &paths {
        let summary = world.summary(path);
        for (_, d) in summary.declarations() {
            let (start, end) = (d.name_range.start().get(), d.name_range.end().get());
            let envelope = world.refs(path, inside(start, end), &cache);
            let want = bound
                .get(&(path.clone(), start))
                .map_or(&[][..], Vec::as_slice);
            tally.targets = tally.targets.saturating_add(1);
            tally.javac = tally.javac.saturating_add(want.len());
            let here = format!("{path}:{start} `{}` {:?}", summary.str(d.name), d.kind);
            tally.check(&here, &envelope, want);
        }
    }
    tally
}

/// Asks the references at every use javac binds to a declaration of `world`, and checks the
/// answer against javac's sites of that declaration.
fn differential_at_uses(world: &mut World, recorded: &Recording) -> Tally {
    let bound = bound_sites(world, recorded);
    let cache = QueryCache::new();
    let mut tally = Tally::default();
    let mut summaries: BTreeMap<String, FileSummary> = BTreeMap::new();
    for ((path, start, end), binding) in &recorded.sites {
        let Binding::Decl {
            kind,
            name,
            path: dpath,
            start: dstart,
            end: dend,
        } = binding
        else {
            continue;
        };
        let summary = summaries
            .entry(dpath.clone())
            .or_insert_with(|| world.summary(dpath));
        let Some(at) = declared(summary, kind, name, *dstart, *dend) else {
            continue;
        };
        let has_site = world
            .summary(path)
            .references()
            .iter()
            .any(|r| r.range.start().get() == *start && r.range.end().get() == *end);
        if !has_site {
            continue;
        }
        let envelope = world.refs(path, inside(*start, *end), &cache);
        let want = bound
            .get(&(dpath.clone(), at))
            .map_or(&[][..], Vec::as_slice);
        tally.targets = tally.targets.saturating_add(1);
        tally.javac = tally.javac.saturating_add(want.len());
        tally.check(&format!("{path}:{start} `{name}`"), &envelope, want);
    }
    tally
}

fn assert_clean(tally: &Tally) {
    assert!(
        tally.missing.is_empty() && tally.wrong.is_empty(),
        "{} missing and {} wrong over {} declarations:\n{}\n{}",
        tally.missing.len(),
        tally.wrong.len(),
        tally.targets,
        tally.missing.join("\n"),
        tally.wrong.join("\n")
    );
}

#[test]
fn should_hold_every_javac_site_and_only_javac_s_as_exact_when_any_declaration_of_shop_is_asked() {
    // given
    let mut world = World::shop();
    let recorded = recording("shop");
    assert_fresh(&world, &recorded, "shop");
    // when
    let tally = differential(&mut world, &recorded);
    // then
    assert_clean(&tally);
    assert!(tally.javac >= SHOP_JAVAC_SITES, "{tally:?}");
    assert!(tally.exact >= SHOP_REFS_EXACT, "{tally:?}");
    assert!(tally.complete >= SHOP_REFS_COMPLETE, "{tally:?}");
}

#[test]
fn should_hold_every_javac_site_and_only_javac_s_as_exact_when_any_declaration_of_the_generated_corpus_is_asked()
 {
    // given
    let mut world = World::generated();
    let recorded = recording("generated");
    assert_fresh(&world, &recorded, "generated");
    // when
    let tally = differential(&mut world, &recorded);
    // then
    assert_clean(&tally);
    assert!(tally.javac >= GENERATED_JAVAC_SITES, "{tally:?}");
    assert!(tally.exact >= GENERATED_REFS_EXACT, "{tally:?}");
}

#[test]
fn should_hold_every_javac_site_of_the_bound_declaration_when_refs_are_asked_at_a_use() {
    // given
    let mut world = World::shop();
    let recorded = recording("shop");
    assert_fresh(&world, &recorded, "shop");
    // when
    let tally = differential_at_uses(&mut world, &recorded);
    // then
    assert_clean(&tally);
    assert!(tally.targets >= SHOP_USES, "{tally:?}");
}

/// How many javac sites bind to `shop`'s declarations: fewer means the project or the recording
/// shrank (floors, as IDX-11 landed: never lowered to get green).
const SHOP_JAVAC_SITES: usize = 184;
/// `exact` references over `shop`'s declarations when IDX-11 landed.
const SHOP_REFS_EXACT: usize = 66;
/// Answers over `shop`'s declarations that claimed `complete` when IDX-11 landed.
const SHOP_REFS_COMPLETE: usize = 176;
/// Uses of `shop` asked.
const SHOP_USES: usize = 183;
/// The same floors for the generated corpus.
const GENERATED_JAVAC_SITES: usize = 601;
const GENERATED_REFS_EXACT: usize = 232;

// The completeness oracle (ver §7.2).

/// An indexer that loses the reference sites of every file holding `marker`: the planted
/// defect of the oracle's own test (a file the index ignores while claiming it indexed).
#[derive(Debug, Clone)]
struct Ignoring {
    inner: JavaIndexer<InlinePool>,
    marker: &'static str,
}

impl SyntaxIndexer for Ignoring {
    fn grammar(&self) -> jfl_application::syntax_indexer::GrammarId {
        self.inner.grammar()
    }

    fn summariser_version(&self) -> u32 {
        self.inner.summariser_version()
    }

    fn index(
        &self,
        bytes: Vec<u8>,
    ) -> impl Future<Output = Result<Indexed, SyntaxIndexError>> + Send {
        let ignore = String::from_utf8_lossy(&bytes).contains(self.marker);
        let indexed = self.inner.index_blocking(&bytes).map(|mut indexed| {
            if ignore {
                indexed.summary = indexed.summary.without_references();
            }
            indexed
        });
        core::future::ready(indexed)
    }
}

/// The name of the class a trial adds.
const PROBE: &str = "ProbeCaller";

/// One trial's outcome.
#[derive(Debug, PartialEq, Eq)]
enum Trial {
    /// The added use is in the answer.
    Found,
    /// The added use is not, and the answer says it is incomplete.
    Incomplete,
    /// The added use is not, and the answer claims `complete`: a counterexample.
    Counterexample(String),
    /// No declaration of a kind the trial can use was picked.
    Skipped,
}

/// Whether `d` is accessible only from its package (the rule `references` applies).
fn package_private(summary: &FileSummary, d: &jfl_domain::Declaration) -> bool {
    let access = Modifiers::PUBLIC
        .with(Modifiers::PROTECTED)
        .with(Modifiers::PRIVATE);
    if d.modifiers.bits() & access.bits() != 0
        || matches!(
            d.kind,
            DeclKind::EnumConstant | DeclKind::RecordComponent | DeclKind::AnnotationElement
        )
    {
        return false;
    }
    d.parent
        .and_then(|p| summary.declaration(p))
        .is_none_or(|p| !matches!(p.kind, DeclKind::Interface | DeclKind::Annotation))
}

/// The use a trial writes of `d`, and where its name starts in it.
fn use_of(summary: &FileSummary, d: &jfl_domain::Declaration) -> Option<(String, usize)> {
    let name = summary.str(d.name);
    let args = |n: usize| vec!["null"; n].join(", ");
    let params = usize::try_from(d.params.len).ok()?;
    let (code, at) = match d.kind {
        DeclKind::Method | DeclKind::RecordComponent | DeclKind::AnnotationElement => {
            let n = if d.kind == DeclKind::Method {
                params
            } else {
                0
            };
            (format!("x.{name}({})", args(n)), 2)
        }
        DeclKind::Field | DeclKind::EnumConstant => (format!("x.{name}"), 2),
        DeclKind::Constructor => {
            let owner = d.parent.and_then(|p| summary.declaration(p))?;
            if owner.kind != DeclKind::Class || owner.modifiers.contains(Modifiers::ABSTRACT) {
                return None;
            }
            (format!("new {name}({})", args(params)), 4)
        }
        kind if kind.is_type() => (format!("({name}) x"), 1),
        _ => return None,
    };
    Some((
        format!("return {code};"),
        "return ".len().saturating_add(at),
    ))
}

/// One seeded trial over `files` (module documentation), indexed by `make`'s indexer.
fn trial<X: SyntaxIndexer>(
    files: &[(String, Vec<u8>)],
    summaries: &[FileSummary],
    seed: u64,
    make: impl Fn() -> X,
) -> Trial {
    let mut rng = Rng(seed);
    let decls: Vec<(usize, jfl_domain::DeclId)> = summaries
        .iter()
        .enumerate()
        .flat_map(|(f, s)| s.declarations().map(move |(id, _)| (f, id)))
        .collect();
    let Some(&(file, id)) = usize::try_from(rng.below(count(decls.len())))
        .ok()
        .and_then(|i| decls.get(i))
    else {
        return Trial::Skipped;
    };
    let (Some(summary), Some((path, _))) = (summaries.get(file), files.get(file)) else {
        return Trial::Skipped;
    };
    let Some(d) = summary.declaration(id) else {
        return Trial::Skipped;
    };
    if d.modifiers.contains(Modifiers::PRIVATE) {
        // Only its top-level type may use it: kept out of the trials, which add a type.
        return Trial::Skipped;
    }
    let Some((body, name_at)) = use_of(summary, d) else {
        return Trial::Skipped;
    };
    // The file the use goes in: any, or one of the declaration's package.
    let package = summary.package().unwrap_or("");
    let hosts: Vec<usize> = (0..files.len())
        .filter(|&f| {
            !package_private(summary, d)
                || summaries
                    .get(f)
                    .is_some_and(|s| s.package().unwrap_or("") == package)
        })
        .collect();
    let Some(&host) = usize::try_from(rng.below(count(hosts.len())))
        .ok()
        .and_then(|i| hosts.get(i))
    else {
        return Trial::Skipped;
    };
    let mut edited = files.to_vec();
    let Some((host_path, host_bytes)) = edited.get_mut(host) else {
        return Trial::Skipped;
    };
    let prefix = format!("\nclass {PROBE}{seed:x} {{\n    Object probe(Object x) {{\n        ");
    let start = host_bytes
        .len()
        .saturating_add(prefix.len())
        .saturating_add(name_at);
    host_bytes.extend_from_slice(format!("{prefix}{body}\n    }}\n}}\n").as_bytes());
    let name = summary.str(d.name);
    let added = (
        host_path.clone(),
        u32::try_from(start).ok(),
        u32::try_from(start.saturating_add(name.len())).ok(),
    );
    let (host_path, Some(added_start), Some(added_end)) = added else {
        return Trial::Skipped;
    };
    let mut world = World::with_indexer(edited, make());
    let offset = inside(d.name_range.start().get(), d.name_range.end().get());
    let envelope = world.refs(path, offset, &QueryCache::new());
    let text = world.text(&host_path);
    let want: Place = (
        host_path.clone(),
        position(text, added_start),
        position(text, added_end),
    );
    let all = places(
        envelope
            .exact()
            .iter()
            .chain(envelope.possible().iter().map(|p| &p.location)),
    );
    if all.contains(&want) {
        Trial::Found
    } else if envelope.complete() {
        Trial::Counterexample(format!(
            "seed {seed:#x}: `{name}` ({:?}) of {path}, used in {host_path}: missing, complete=true",
            d.kind
        ))
    } else {
        Trial::Incomplete
    }
}

/// The trials of `seeds` over `files`.
fn oracle<X: SyntaxIndexer>(
    files: &[(String, Vec<u8>)],
    seeds: core::ops::Range<u64>,
    make: impl Fn() -> X + Copy,
) -> Vec<Trial> {
    let indexer = indexer();
    let summaries: Vec<FileSummary> = files
        .iter()
        .map(|(_, b)| {
            indexer
                .index_blocking(b)
                .map(|i| i.summary)
                .unwrap_or_default()
        })
        .collect();
    seeds
        .map(|seed| trial(files, &summaries, ORACLE_SEED ^ seed, make))
        .collect()
}

/// `n` as a bound for [`Rng::below`].
fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// The oracle's base seed; each trial XORs its number in.
const ORACLE_SEED: u64 = 0x1D11_2026_1124_0001;

/// Trials per project in the gate's budget.
const TRIALS: u64 = 48;

fn summarise(trials: &[Trial]) -> (usize, usize, Vec<String>) {
    let found = trials.iter().filter(|t| **t == Trial::Found).count();
    let incomplete = trials.iter().filter(|t| **t == Trial::Incomplete).count();
    let counter = trials
        .iter()
        .filter_map(|t| match t {
            Trial::Counterexample(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    (found, incomplete, counter)
}

#[test]
fn should_find_every_added_use_or_say_incomplete_when_the_oracle_adds_one_to_each_project() {
    // given
    let projects = [("shop", shop_files()), ("generated", generated_files())];
    for (name, files) in &projects {
        // when
        let trials = oracle(files, 0..TRIALS, indexer);
        // then
        let (found, incomplete, counter) = summarise(&trials);
        assert!(counter.is_empty(), "{name}: {}", counter.join("\n"));
        assert!(
            found >= ORACLE_FOUND,
            "{name}: only {found} of {TRIALS} trials found their use ({incomplete} incomplete)"
        );
    }
}

#[test]
fn should_find_a_counterexample_when_the_index_ignores_the_edited_file() {
    // given: the planted defect, an index that drops the edited file's sites and calls it
    // indexed
    let files = shop_files();
    let ignoring = || Ignoring {
        inner: indexer(),
        marker: PROBE,
    };
    // when
    let trials = oracle(&files, 0..TRIALS, ignoring);
    // then
    let (found, _, counter) = summarise(&trials);
    assert_eq!(found, 0);
    assert!(
        !counter.is_empty(),
        "the oracle saw no wrong complete=true under the planted defect"
    );
}

/// Trials that must find their use: fewer means the trials stopped testing anything (every
/// answer incomplete or skipped).
const ORACLE_FOUND: usize = 40;

/// `shop` with `extra` files added, and the references of `Tally.post(int)` over it.
fn tally_post_with(extra: &[(&str, &str)]) -> Envelope {
    let mut files = shop_files();
    for (path, text) in extra {
        files.push(((*path).to_owned(), text.as_bytes().to_vec()));
    }
    files.sort();
    let mut world = World::new(files);
    let path = "src/main/java/com/acme/refs/Tally.java";
    let at = world.text(path).find("post(int amount)").unwrap();
    world.refs(
        path,
        u32::try_from(at).unwrap().saturating_add(1),
        &QueryCache::new(),
    )
}

#[test]
fn should_say_generated_sources_unknown_when_a_file_uses_a_known_annotation_processor() {
    // given: a file of the view that imports Lombok
    let lombok = (
        "src/main/java/com/acme/refs/Mapped.java",
        "package com.acme.refs;\n\nimport lombok.Getter;\n\n@Getter\nclass Mapped {\n    int weight;\n}\n",
    );
    // when
    let plain = tally_post_with(&[]);
    let processed = tally_post_with(&[lombok]);
    // then
    assert!(plain.complete(), "{:?}", plain.incomplete_because());
    assert_eq!(
        processed.incomplete_because(),
        [jfl_domain::IncompleteReason::GeneratedSourcesUnknown { files: 1 }]
    );
}

#[test]
fn should_leave_every_site_possible_and_say_declared_twice_when_a_type_is_in_two_source_sets() {
    // given: a second `com.acme.refs.Tally`, as a test source set would hold
    let copy = (
        "src/test/java/com/acme/refs/Tally.java",
        include_str!(
            "../../../../fixtures/java/projects/shop/src/main/java/com/acme/refs/Tally.java"
        ),
    );
    // when
    let once = tally_post_with(&[]);
    let twice = tally_post_with(&[copy]);
    // then: the sites proved once are only possible now
    assert!(!once.exact().is_empty());
    assert!(twice.exact().is_empty());
    assert!(twice.possible().len() >= once.exact().len() + once.possible().len());
    assert_eq!(
        twice.incomplete_because(),
        [jfl_domain::IncompleteReason::TargetDeclaredTwice]
    );
}

#[test]
fn should_say_syntax_errors_when_a_file_of_the_view_does_not_parse() {
    // given: a file of the view with a syntax error, whose recovery may lose a use of `post`
    let broken = (
        "src/main/java/com/acme/refs/Broken.java",
        "package com.acme.refs;\n\nclass Broken {\n    void f(Tally t) {\n        t.post(1\n    }\n}\n",
    );
    let mut files = shop_files();
    files.push((broken.0.to_owned(), broken.1.as_bytes().to_vec()));
    files.sort();
    let mut world = World::new(files);
    let path = "src/main/java/com/acme/refs/Tally.java";
    let at = u32::try_from(world.text(path).find("post(int amount)").unwrap())
        .unwrap()
        .saturating_add(1);
    // when
    let references = world.refs(path, at, &QueryCache::new());
    let definition = world.def(path, at, &QueryCache::new());
    // then: both answers withdraw `complete` for it
    let reason = jfl_domain::IncompleteReason::SyntaxErrors { files: 1 };
    assert_eq!(
        references.incomplete_because(),
        core::slice::from_ref(&reason)
    );
    assert_eq!(definition.incomplete_because(), [reason]);
}

#[test]
fn should_count_build_scripts_and_build_src_only_for_build_logic_when_a_view_has_them() {
    // given: Groovy build scripts, a Groovy source in `buildSrc` and a Java plugin beside it,
    // which only the build's scripts and `buildSrc` can see
    let plugin = "buildSrc/src/main/java/conventions/Conventions.java";
    let mut files = shop_files();
    for (path, text) in [
        ("build.gradle", "plugins { id 'java' }\n"),
        ("settings.gradle", "rootProject.name = 'shop'\n"),
        (
            "buildSrc/src/main/groovy/conventions/Lock.groovy",
            "package conventions\nclass Lock {}\n",
        ),
        (
            plugin,
            "package conventions;\n\npublic class Conventions {\n    public void pin(int level) {}\n}\n",
        ),
    ] {
        files.push((path.to_owned(), text.as_bytes().to_vec()));
    }
    files.sort();
    let mut world = World::new(files);
    let tally = "src/main/java/com/acme/refs/Tally.java";
    let post = u32::try_from(world.text(tally).find("post(int amount)").unwrap())
        .unwrap()
        .saturating_add(1);
    let pin = u32::try_from(world.text(plugin).find("pin(int level)").unwrap())
        .unwrap()
        .saturating_add(1);
    // when
    let main = world.refs(tally, post, &QueryCache::new());
    let build_logic = world.refs(plugin, pin, &QueryCache::new());
    // then: the main source set's target stays complete; the plugin's counts all three scripts'
    // and sources' files
    assert!(main.complete(), "{:?}", main.incomplete_because());
    assert_eq!(
        build_logic.incomplete_because(),
        [jfl_domain::IncompleteReason::OtherLanguageSources { files: 3 }]
    );
}
