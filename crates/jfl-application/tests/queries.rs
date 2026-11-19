//! Component tests of the queries (IDX-10): definition, hover, document and workspace symbols
//! over the fixture project `fixtures/java/projects/shop`, served by the view service over the
//! testkit's fakes (a fake tree, a scripted repository, the canned indexer answering with the
//! project's golden summaries, an in-memory store, a fake wall clock). Answers chosen for M1's
//! golden cases are compared with `tests/fixtures/queries/`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use jfl_application::queries::{definition, document_symbols, hover, workspace_symbols};
use jfl_application::syntax_indexer::GrammarId;
use jfl_application::{
    Answer, ClockDeadline, CountingClock, Pinned, QueryCache, QueryConfig, QueryError,
    ServiceConfig, SymbolFilter, SymbolQuery, ViewService,
};
use jfl_domain::{
    DeclKind, Envelope, FileUri, Freshness, IncompleteReason, LineIndex, Position,
    PositionEncoding, Reason, TargetStatus, UnixMillis,
};
use jfl_testkit::exec::block_on;
use jfl_testkit::fakes::clock::FakeWallClock;
use jfl_testkit::fakes::hasher::FnvHasher;
use jfl_testkit::fakes::repo_probe::FakeRepoProbe;
use jfl_testkit::fakes::source_tree::FakeSourceTree;
use jfl_testkit::fakes::summary_store::{Backing, MemorySummaryStore};
use jfl_testkit::fakes::syntax_indexer::CannedSyntaxIndexer;
use jfl_testkit::{golden, query_golden};

#[cfg(test)]
mod given {
    use super::*;

    pub(crate) const PROJECT: &str = "projects/shop";
    pub(crate) const MAIN: &str = "/w/m";
    pub(crate) const SRC: &str = "src/main/java/com/acme";
    pub(crate) const UTF16: PositionEncoding = PositionEncoding::Utf16;

    pub(crate) type Service = ViewService<
        FakeSourceTree,
        FakeRepoProbe,
        CannedSyntaxIndexer,
        MemorySummaryStore,
        FakeWallClock,
        FnvHasher,
    >;

    /// The fixture project at `/w/m`, its files written to the tree and committed.
    pub(crate) struct World {
        pub(crate) tree: FakeSourceTree,
        pub(crate) probe: FakeRepoProbe,
        pub(crate) indexer: CannedSyntaxIndexer,
        pub(crate) files: Vec<(String, Vec<u8>)>,
    }

    pub(crate) fn uri(path: &str) -> FileUri {
        FileUri::from_path(path).unwrap()
    }

    /// `/w/m/src/main/java/com/acme/<file>`.
    pub(crate) fn source(file: &str) -> String {
        format!("{MAIN}/{SRC}/{file}")
    }

    impl World {
        pub(crate) fn new() -> Self {
            let tree = FakeSourceTree::new(uri("/w"), 1024);
            let probe = FakeRepoProbe::new();
            let mut files = Vec::new();
            for file in golden::corpus(PROJECT).unwrap() {
                let path = file
                    .path
                    .strip_prefix(&format!("{PROJECT}/"))
                    .unwrap()
                    .to_owned();
                tree.write(&format!("m/{path}"), &file.bytes).unwrap();
                files.push((path, file.bytes));
            }
            let committed: Vec<(&str, &[u8])> = files
                .iter()
                .map(|(p, b)| (p.as_str(), b.as_slice()))
                .collect();
            probe.init(&uri(MAIN), &committed).unwrap();
            let grammar = GrammarId {
                name: "tree-sitter-java-orchard",
                version: "0.5.25",
            };
            let indexer = CannedSyntaxIndexer::from_goldens(grammar, "orchard", PROJECT).unwrap();
            Self {
                tree,
                probe,
                indexer,
                files,
            }
        }

        pub(crate) fn service(&self) -> Service {
            self.service_with(ServiceConfig::new("t1"))
        }

        pub(crate) fn service_with(&self, config: ServiceConfig) -> Service {
            let store = MemorySummaryStore::open(
                Backing::new(),
                1 << 30,
                Arc::new(CountingClock::starting_at(1)),
            );
            ViewService::new(
                (
                    self.tree.clone(),
                    self.probe.clone(),
                    self.indexer.clone(),
                    store,
                ),
                FakeWallClock::at(1_800_000_000),
                FnvHasher::default(),
                config,
            )
        }

        /// The text of `src/main/java/com/acme/<file>`.
        pub(crate) fn text(&self, file: &str) -> String {
            let path = format!("{SRC}/{file}");
            let (_, bytes) = self.files.iter().find(|(p, _)| *p == path).unwrap();
            String::from_utf8(bytes.clone()).unwrap()
        }

        /// The UTF-16 position of the `nth` (0-based) occurrence of `needle` in `file`, moved
        /// `skip` bytes into it.
        pub(crate) fn at(&self, file: &str, needle: &str, nth: usize, skip: usize) -> Position {
            let text = self.text(file);
            let offset = text
                .match_indices(needle)
                .nth(nth)
                .unwrap()
                .0
                .checked_add(skip)
                .unwrap();
            let lines = LineIndex::new(&text).unwrap();
            lines
                .position(
                    jfl_domain::ByteOffset::new(u32::try_from(offset).unwrap()),
                    UTF16,
                )
                .unwrap()
        }
    }

    pub(crate) fn ask(service: &mut Service, file: &str) -> Answer {
        block_on(service.request(&uri(&source(file)))).unwrap()
    }

    pub(crate) fn pinned(answer: &Answer) -> Pinned<'_> {
        Pinned {
            answer,
            head: None,
            freshness: Freshness {
                synced_at: Some(UnixMillis(1_800_000_000_000)),
                pending_events: 0,
            },
        }
    }

    /// A clock that never moves: no deadline passes.
    pub(crate) fn still() -> FakeWallClock {
        FakeWallClock::at(1_800_000_000)
    }

    pub(crate) fn def(
        service: &Service,
        answer: &Answer,
        position: Position,
    ) -> Result<Envelope, QueryError> {
        let clock = still();
        let config = QueryConfig::default();
        let deadline = ClockDeadline::new(&clock, config.definition_budget);
        block_on(definition(
            &pinned(answer),
            &service.sources(),
            position,
            UTF16,
            &config,
            &deadline,
            &QueryCache::new(),
        ))
    }

    /// The `(file name, line)` of each location, 0-based lines.
    pub(crate) fn places(locations: &[jfl_domain::Location]) -> Vec<(String, u32)> {
        locations
            .iter()
            .map(|l| {
                let name = l.uri.path().rsplit('/').next().unwrap().to_owned();
                (name, l.range.start().line())
            })
            .collect()
    }

    /// The one candidate the scope rules bound, as `admit` lets it through: no target, no exact
    /// hit, one `possible` hit with the scope rules' reason, and `TargetUnresolved`.
    pub(crate) fn candidate(envelope: &Envelope) -> Option<(String, u32)> {
        let gated = envelope.target().is_none()
            && envelope.exact().is_empty()
            && envelope
                .incomplete_because()
                .contains(&IncompleteReason::TargetUnresolved)
            && envelope.possible().iter().all(|p| {
                p.why.to_string() == "matched by name and arity (bound by scope rules read from syntax, not proved)"
            });
        match (gated, possible_places(envelope).as_slice()) {
            (true, [one]) => Some(one.clone()),
            _ => None,
        }
    }

    pub(crate) fn possible_places(envelope: &Envelope) -> Vec<(String, u32)> {
        let locations: Vec<jfl_domain::Location> = envelope
            .possible()
            .iter()
            .map(|p| p.location.clone())
            .collect();
        places(&locations)
    }

    pub(crate) fn target(envelope: &Envelope) -> Option<(String, TargetStatus)> {
        envelope.target().map(|t| (t.symbol.to_string(), t.status))
    }

    pub(crate) fn goldens() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/queries")
    }

    /// Checks `envelope`, asked at `position` of `file`, against the golden `name`.
    pub(crate) fn golden_def(name: &str, file: &str, position: Position, envelope: &Envelope) {
        let text = query_golden::definition(envelope, &uri(&source(file)), position).unwrap();
        query_golden::check(&goldens(), name, &text).unwrap();
    }
}

use given::*;

#[test]
fn should_offer_the_static_method_as_the_only_candidate_when_its_receiver_is_an_imported_type() {
    // given: `Billing.standard()`; the scope rules bind it, and `admit` keeps it `possible`
    let world = World::new();
    let mut service = world.service();
    let answer = ask(&mut service, "shop/Shop.java");
    let at = world.at("shop/Shop.java", "standard", 0, 2);
    // when
    let envelope = def(&service, &answer, at).unwrap();
    // then
    assert_eq!(candidate(&envelope), Some(("Billing.java".to_owned(), 32)));
    assert!(!envelope.complete());
    golden_def("def.static-call", "shop/Shop.java", at, &envelope);
}

#[test]
fn should_answer_the_declaration_itself_when_the_cursor_is_on_its_name() {
    // given
    let world = World::new();
    let mut service = world.service();
    let answer = ask(&mut service, "billing/Billing.java");
    let at = world.at("billing/Billing.java", "standard()", 0, 0);
    // when
    let envelope = def(&service, &answer, at).unwrap();
    // then
    assert_eq!(target(&envelope).map(|t| t.1), Some(TargetStatus::Exact));
    assert_eq!(places(envelope.exact()), [("Billing.java".to_owned(), 32)]);
    assert!(envelope.complete());
}

#[test]
fn should_list_every_overload_of_the_owner_and_no_other_type_s_when_the_overload_is_not_proved() {
    // given: Billing declares three `charge`; Cart declares one of the same name
    let world = World::new();
    let mut service = world.service();
    let answer = ask(&mut service, "billing/Billing.java");
    let at = world.at("billing/Billing.java", "charge(amount)", 0, 1);
    // when
    let envelope = def(&service, &answer, at).unwrap();
    // then
    assert_eq!(envelope.target(), None);
    assert!(envelope.exact().is_empty());
    assert_eq!(
        possible_places(&envelope),
        [
            ("Billing.java".to_owned(), 11),
            ("Billing.java".to_owned(), 18),
            ("Billing.java".to_owned(), 23),
        ]
    );
    assert!(
        envelope
            .possible()
            .iter()
            .all(|p| p.why.reason() == Reason::OverloadNotProved)
    );
    assert_eq!(
        envelope.incomplete_because(),
        [IncompleteReason::TargetUnresolved]
    );
    golden_def(
        "def.bounded-ambiguity",
        "billing/Billing.java",
        at,
        &envelope,
    );
}

#[test]
fn should_list_inherited_overloads_and_never_another_type_s_when_a_subclass_calls_one() {
    // given: PremiumBilling extends Billing (a same-package type, so not proved: H6); Cart's
    // `charge` is another type's member
    let world = World::new();
    let mut service = world.service();
    let answer = ask(&mut service, "billing/PremiumBilling.java");
    let at = world.at("billing/PremiumBilling.java", "charge(amount)", 0, 1);
    // when
    let envelope = def(&service, &answer, at).unwrap();
    // then
    let places = possible_places(&envelope);
    assert_eq!(
        places,
        [
            ("Billing.java".to_owned(), 11),
            ("Billing.java".to_owned(), 18),
            ("Billing.java".to_owned(), 23),
            ("PremiumBilling.java".to_owned(), 7),
        ]
    );
    assert!(!places.iter().any(|(file, _)| file == "Cart.java"));
    assert_eq!(envelope.target(), None);
    assert_eq!(
        envelope.incomplete_because(),
        [IncompleteReason::TargetUnresolved]
    );
}

#[test]
fn should_offer_one_candidate_when_one_overload_fits_or_the_member_is_inherited_through_an_import()
{
    // given: Discount (com.acme.shop) extends Billing through a single-type import
    let world = World::new();
    let mut service = world.service();
    let billing = ask(&mut service, "billing/Billing.java");
    let discount = ask(&mut service, "shop/Discount.java");
    let log = world.at("billing/Billing.java", "log(amount)", 0, 1);
    let inherited = world.at("shop/Discount.java", "standard()", 0, 1);
    let field = world.at("billing/Billing.java", "return RETRIES", 0, 8);
    // when
    let found = [
        def(&service, &billing, log).unwrap(),
        def(&service, &discount, inherited).unwrap(),
        def(&service, &billing, field).unwrap(),
    ];
    // then: the scope rules' one candidate each (javac binds the same), never `exact`
    let candidates: Vec<_> = found.iter().map(candidate).collect();
    assert_eq!(
        candidates,
        [
            Some(("Billing.java".to_owned(), 40)),
            Some(("Billing.java".to_owned(), 32)),
            Some(("Billing.java".to_owned(), 6)),
        ]
    );
    golden_def(
        "def.inherited-method",
        "shop/Discount.java",
        inherited,
        &found[1],
    );
}

#[test]
fn should_keep_a_same_package_supertype_s_member_possible_when_it_is_inherited() {
    // given: PremiumBilling extends Billing, found only by the same-package step (H6)
    let world = World::new();
    let mut service = world.service();
    let premium = ask(&mut service, "billing/PremiumBilling.java");
    let retries = world.at("billing/PremiumBilling.java", "retries()", 0, 1);
    // when
    let envelope = def(&service, &premium, retries).unwrap();
    // then: javac's `Billing#retries()` is the candidate, not proved
    assert!(envelope.exact().is_empty());
    assert_eq!(
        possible_places(&envelope),
        [("Billing.java".to_owned(), 36)]
    );
    assert_eq!(
        envelope.incomplete_because(),
        [IncompleteReason::TargetUnresolved]
    );
    golden_def(
        "def.same-package-supertype",
        "billing/PremiumBilling.java",
        retries,
        &envelope,
    );
}

#[test]
fn should_offer_the_bound_declaration_as_the_only_candidate_when_imports_and_receivers_name_it() {
    // given
    let world = World::new();
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    let checkout = ask(&mut service, "shop/Checkout.java");
    let cases = [
        (
            &shop,
            world.at("shop/Shop.java", "Billing billing", 0, 1),
            ("Billing.java", 5),
        ),
        (
            &shop,
            world.at("shop/Shop.java", "Receipt checkout", 0, 1),
            ("Receipt.java", 5),
        ),
        (
            &checkout,
            world.at("shop/Checkout.java", "standard()", 0, 1),
            ("Billing.java", 32),
        ),
        (
            &shop,
            world.at("shop/Shop.java", "this.audit", 0, 6),
            ("Shop.java", 22),
        ),
    ];
    for (answer, at, (file, line)) in cases {
        // when
        let envelope = def(&service, answer, at).unwrap();
        // then
        assert_eq!(
            candidate(&envelope),
            Some((file.to_owned(), line)),
            "{file}"
        );
    }
}

#[test]
fn should_keep_same_package_and_on_demand_types_possible_when_nothing_else_names_them() {
    // given: Cart is in Shop's package; Money comes through `import com.acme.money.*`. Either
    // may be shadowed by a type the view does not hold (a pending file, generated sources) or
    // be in a source set Shop cannot see, until the project model (PROJ).
    let world = World::new();
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    let billing = ask(&mut service, "billing/Billing.java");
    let cases = [
        (
            &shop,
            world.at("shop/Shop.java", "Cart cart", 0, 1),
            "Cart.java",
            "same package",
        ),
        (
            &shop,
            world.at("shop/Shop.java", "Money amount", 0, 1),
            "Money.java",
            "on-demand",
        ),
        (
            &billing,
            world.at("billing/Billing.java", "new Receipt", 0, 5),
            "Receipt.java",
            "same package",
        ),
    ];
    for (answer, at, file, why) in cases {
        // when
        let envelope = def(&service, answer, at).unwrap();
        // then
        assert_eq!(envelope.target(), None, "{file}");
        assert!(envelope.exact().is_empty(), "{file}");
        assert_eq!(possible_places(&envelope).len(), 1, "{file}");
        assert_eq!(possible_places(&envelope)[0].0, file);
        assert!(
            envelope.possible()[0].why.to_string().contains(why),
            "{file}"
        );
        assert!(!envelope.complete(), "{file}");
    }
}

#[test]
fn should_offer_the_constructor_of_that_arity_when_a_type_is_created() {
    // given
    let world = World::new();
    let mut service = world.service();
    let cart = ask(&mut service, "shop/Cart.java");
    let money = ask(&mut service, "money/Money.java");
    let cases = [
        (&cart, world.at("shop/Cart.java", "new Money", 0, 5)),
        (&money, world.at("money/Money.java", "this(units", 0, 1)),
    ];
    for (answer, at) in cases {
        // when
        let envelope = def(&service, answer, at).unwrap();
        // then: `Money(long)`
        assert_eq!(candidate(&envelope), Some(("Money.java".to_owned(), 7)));
    }
}

#[test]
fn should_say_why_when_a_definition_finds_nothing() {
    // given: `List` is a library type; Ledger's `add` comes from ArrayList, outside the index
    let world = World::new();
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    let ledger = ask(&mut service, "shop/Ledger.java");
    let list = world.at("shop/Shop.java", "List<Receipt>", 0, 1);
    let add = world.at("shop/Ledger.java", "add(amount)", 0, 1);
    // when
    let outside = def(&service, &shop, list).unwrap();
    let unresolved = def(&service, &ledger, add).unwrap();
    // then
    for envelope in [&outside, &unresolved] {
        assert!(envelope.exact().is_empty() && envelope.possible().is_empty());
        assert!(!envelope.complete());
    }
    assert_eq!(
        outside.incomplete_because(),
        [IncompleteReason::TargetNotIndexed]
    );
    let import = def(&service, &shop, world.at("shop/Shop.java", "List;", 0, 1)).unwrap();
    assert_eq!(
        import.incomplete_because(),
        [IncompleteReason::TargetNotIndexed]
    );
    assert_eq!(
        unresolved.incomplete_because(),
        [IncompleteReason::TargetUnresolved]
    );
    golden_def("def.outside-index", "shop/Shop.java", list, &outside);
    golden_def(
        "def.inherited-from-library",
        "shop/Ledger.java",
        add,
        &unresolved,
    );
}

#[test]
fn should_never_claim_exact_when_a_supertype_outside_the_index_may_shadow_the_name() {
    // given: Ledger extends ArrayList, which may declare a member type `Money` or more `sum`s
    let world = World::new();
    let mut service = world.service();
    let ledger = ask(&mut service, "shop/Ledger.java");
    let money = world.at("shop/Ledger.java", "Money sum", 0, 1);
    let sum = world.at("shop/Ledger.java", "sum();", 0, 1);
    // when
    let shadowed = def(&service, &ledger, money).unwrap();
    let overloads = def(&service, &ledger, sum).unwrap();
    // then
    assert!(shadowed.exact().is_empty());
    assert_eq!(possible_places(&shadowed), [("Money.java".to_owned(), 3)]);
    assert_eq!(
        shadowed.possible()[0].why.reason(),
        Reason::SupertypeNotIndexed
    );
    assert_eq!(
        shadowed.incomplete_because(),
        [IncompleteReason::TargetUnresolved]
    );
    assert!(overloads.exact().is_empty());
    assert_eq!(possible_places(&overloads), [("Ledger.java".to_owned(), 7)]);
    assert_eq!(overloads.target(), None);
    assert!(!overloads.complete());
    golden_def(
        "def.supertype-not-indexed",
        "shop/Ledger.java",
        money,
        &shadowed,
    );
}

#[test]
fn should_never_claim_exact_when_the_site_is_inside_an_anonymous_class() {
    // given: `size()` inside `new Runnable() { … }`, whose supertype is outside the index
    let world = World::new();
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    let at = world.at("shop/Shop.java", "    size();", 0, 5);
    // when
    let envelope = def(&service, &shop, at).unwrap();
    // then
    assert!(envelope.exact().is_empty());
    assert_eq!(possible_places(&envelope), [("Shop.java".to_owned(), 26)]);
    assert_eq!(
        envelope.possible()[0].why.reason(),
        Reason::SupertypeNotIndexed
    );
    assert!(!envelope.complete());
    golden_def("def.anonymous-class", "shop/Shop.java", at, &envelope);
}

#[test]
fn should_list_candidates_by_name_and_arity_when_the_receiver_type_is_unknown() {
    // given: `billing.charge(…)`, a field receiver whose type syntax does not prove
    let world = World::new();
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    let at = world.at("shop/Shop.java", "charge(cart", 0, 1);
    // when
    let envelope = def(&service, &shop, at).unwrap();
    // then
    assert_eq!(envelope.target(), None);
    assert_eq!(
        possible_places(&envelope),
        [
            ("Billing.java".to_owned(), 11),
            ("Billing.java".to_owned(), 23),
            ("Cart.java".to_owned(), 14),
        ]
    );
    assert!(
        envelope
            .possible()
            .iter()
            .all(|p| p.why.reason() == Reason::ReceiverTypeUnknown)
    );
    assert_eq!(
        envelope.incomplete_because(),
        [IncompleteReason::TargetUnresolved]
    );
    golden_def("def.receiver-unknown", "shop/Shop.java", at, &envelope);
}

#[test]
fn should_read_a_receiver_as_the_field_when_a_field_obscures_a_type_of_its_name() {
    // given: `Billing Billing` in Obscure; `Billing.charge(…)` calls the field's method (JLS 6.4.2)
    let world = World::new();
    let mut service = world.service();
    let obscure = ask(&mut service, "shop/Obscure.java");
    let call = world.at("shop/Obscure.java", "charge(amount)", 0, 1);
    let created = world.at("shop/Obscure.java", "new Billing", 0, 5);
    // when
    let envelope = def(&service, &obscure, call).unwrap();
    let constructor = def(&service, &obscure, created).unwrap();
    // then
    assert_eq!(envelope.target(), None);
    assert!(
        envelope
            .possible()
            .iter()
            .all(|p| p.why.reason() == Reason::ReceiverTypeUnknown)
    );
    assert!(!envelope.complete());
    assert_eq!(
        candidate(&constructor),
        Some(("Billing.java".to_owned(), 5))
    );
}

#[test]
fn should_answer_unresolved_when_the_name_is_a_type_variable() {
    // given
    let world = World::new();
    let mut service = world.service();
    let cart = ask(&mut service, "shop/Cart.java");
    let at = world.at("shop/Cart.java", "T first", 0, 0);
    // when
    let envelope = def(&service, &cart, at).unwrap();
    // then
    assert!(envelope.exact().is_empty() && envelope.possible().is_empty());
    assert_eq!(
        envelope.incomplete_because(),
        [IncompleteReason::TargetUnresolved]
    );
}

#[test]
fn should_refuse_when_no_name_is_at_the_position_or_it_is_past_the_end() {
    // given
    let world = World::new();
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    let keyword = world.at("shop/Shop.java", "return receipt", 0, 1);
    // when
    let none = def(&service, &shop, keyword);
    let past = def(&service, &shop, Position::new(4000, 0));
    // then
    assert_eq!(none, Err(QueryError::NoSymbol));
    assert!(matches!(past, Err(QueryError::Position(_))));
}

#[test]
fn should_answer_incomplete_with_deadline_when_the_clock_passes_it() {
    // given: clocks that move past each deadline (300 ms for definition, 2 s for symbols) within
    // a few readings
    let world = World::new();
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    let at = world.at("shop/Shop.java", "standard", 0, 2);
    let clock = FakeWallClock::stepping(1_800_000_000, Duration::from_millis(400));
    let slow = FakeWallClock::stepping(1_800_000_000, Duration::from_secs(3));
    let config = QueryConfig::default();
    // when
    let envelope = block_on(definition(
        &pinned(&shop),
        &service.sources(),
        at,
        UTF16,
        &config,
        &ClockDeadline::new(&clock, config.definition_budget),
        &QueryCache::new(),
    ))
    .unwrap();
    let symbols = block_on(workspace_symbols(
        &pinned(&shop),
        &service.sources(),
        SymbolQuery {
            text: "Billing",
            filter: None,
            limit: None,
        },
        UTF16,
        &config,
        &ClockDeadline::new(&slow, config.symbols_budget),
        &QueryCache::new(),
    ));
    // then
    assert!(!envelope.complete());
    assert!(
        envelope
            .incomplete_because()
            .contains(&IncompleteReason::Deadline)
    );
    assert!(!symbols.completeness.is_complete());
    assert!(
        symbols
            .completeness
            .reasons()
            .contains(&IncompleteReason::Deadline)
    );
    golden_def("def.deadline", "shop/Shop.java", at, &envelope);
}

#[test]
fn should_answer_partly_indexed_when_the_view_defers_files_at_its_open() {
    // given: a view opened with only the request's directory read
    let world = World::new();
    let mut config = ServiceConfig::new("t1");
    config.open_whole = 1;
    let mut service = world.service_with(config);
    let shop = ask(&mut service, "shop/Shop.java");
    let at = world.at("shop/Shop.java", "standard", 0, 2);
    // when
    let envelope = def(&service, &shop, at).unwrap();
    // then
    assert!(!envelope.complete());
    assert!(
        envelope
            .incomplete_because()
            .iter()
            .any(|r| matches!(r, IncompleteReason::ModulePartlyIndexed { .. }))
    );
}

#[test]
fn should_answer_not_watched_when_the_filesystem_cannot_be_watched() {
    // given
    let world = World::new();
    world
        .tree
        .set_filesystem(jfl_application::source_tree::FsKind::Network);
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    let at = world.at("shop/Shop.java", "standard", 0, 2);
    // when
    let envelope = def(&service, &shop, at).unwrap();
    // then
    assert_eq!(candidate(&envelope), Some(("Billing.java".to_owned(), 32)));
    assert!(
        envelope
            .incomplete_because()
            .contains(&IncompleteReason::NotWatched)
    );
}

#[test]
fn should_leave_out_a_file_and_say_so_when_it_changed_after_the_pin() {
    // given: Billing.java is rewritten after the request pinned its generation
    let world = World::new();
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    let at = world.at("shop/Shop.java", "standard", 0, 2);
    let changed = format!("// moved\n{}", world.text("billing/Billing.java"));
    world
        .tree
        .write(&format!("m/{SRC}/billing/Billing.java"), changed.as_bytes())
        .unwrap();
    // when
    let envelope = def(&service, &shop, at).unwrap();
    // then
    assert!(envelope.exact().is_empty());
    assert!(
        envelope
            .incomplete_because()
            .contains(&IncompleteReason::ModuleStale {
                module: jfl_domain::ModuleName::worktree(),
                stale: 1,
            })
    );
}

#[test]
fn should_describe_the_declaration_when_hovering_its_name_and_say_unresolved_at_a_use() {
    // given
    let world = World::new();
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    let billing = ask(&mut service, "billing/Billing.java");
    let clock = still();
    let config = QueryConfig::default();
    let deadline = ClockDeadline::new(&clock, config.definition_budget);
    let cache = QueryCache::new();
    let run = |answer, at| {
        block_on(hover(
            &pinned(answer),
            &service.sources(),
            at,
            UTF16,
            &config,
            &deadline,
            &cache,
        ))
        .unwrap()
    };
    // when
    let declared = run(
        &billing,
        world.at("billing/Billing.java", "standard()", 0, 2),
    );
    let used = run(&shop, world.at("shop/Shop.java", "standard", 0, 2));
    let outside = run(&shop, world.at("shop/Shop.java", "List<Receipt>", 0, 1));
    // then
    assert_eq!(declared.status_line(), "exact");
    assert_eq!(declared.declarations.len(), 1);
    assert_eq!(
        declared.declarations[0].signature,
        "public static Billing standard()"
    );
    assert_eq!(
        declared.declarations[0].documentation.as_deref(),
        Some("The billing every shop starts with.")
    );
    assert_eq!(declared.module.as_str(), ":");
    assert!(declared.range.is_some());
    assert_eq!(
        used.status_line(),
        "unresolved: target not resolved from syntax"
    );
    assert!(used.declarations.is_empty());
    assert_eq!(
        outside.status_line(),
        "unresolved: target declared outside the indexed sources"
    );
    assert!(outside.declarations.is_empty());
}

#[test]
fn should_list_a_document_s_declarations_when_asked_for_its_symbols() {
    // given
    let world = World::new();
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    // when
    let symbols = block_on(document_symbols(&pinned(&shop), &service.sources(), UTF16)).unwrap();
    // then
    let names: Vec<(&str, Option<usize>)> = symbols
        .symbols
        .iter()
        .map(|s| (s.name.as_str(), s.parent))
        .collect();
    assert_eq!(
        names,
        [
            ("Shop", None),
            ("billing", Some(0)),
            ("receipts", Some(0)),
            ("checkout", Some(0)),
            ("audit", Some(0)),
            ("size", Some(0)),
            ("task", Some(0)),
            ("Counter", Some(0)),
            ("count", Some(7)),
            ("bump", Some(7)),
        ]
    );
    assert_eq!(symbols.symbols[0].selection.start(), Position::new(9, 13));
    assert!(symbols.completeness.is_complete());
}

#[test]
fn should_rank_and_cap_workspace_symbols_when_a_query_matches_several() {
    // given
    let world = World::new();
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    let clock = still();
    let config = QueryConfig::default();
    let deadline = ClockDeadline::new(&clock, config.symbols_budget);
    let cache = QueryCache::new();
    let run = |text, filter, limit| {
        block_on(workspace_symbols(
            &pinned(&shop),
            &service.sources(),
            SymbolQuery {
                text,
                filter,
                limit,
            },
            UTF16,
            &config,
            &deadline,
            &cache,
        ))
    };
    // when
    let all = run("Billing", None, None);
    let types = run("Billing", Some(SymbolFilter::Type), None);
    let capped = run("Billing", None, Some(1));
    let humps = run("PB", None, None);
    // then
    let names = |s: &jfl_application::Symbols| -> Vec<(String, DeclKind)> {
        s.symbols.iter().map(|w| (w.name.clone(), w.kind)).collect()
    };
    assert_eq!(
        names(&all),
        [
            ("Billing".to_owned(), DeclKind::Class),
            ("Billing".to_owned(), DeclKind::Field),
            ("billing".to_owned(), DeclKind::Field),
            ("PremiumBilling".to_owned(), DeclKind::Class),
        ]
    );
    assert!(all.completeness.is_complete());
    assert_eq!(
        names(&types),
        [
            ("Billing".to_owned(), DeclKind::Class),
            ("PremiumBilling".to_owned(), DeclKind::Class),
        ]
    );
    assert_eq!(names(&capped), [("Billing".to_owned(), DeclKind::Class)]);
    assert_eq!(
        capped.completeness.reasons(),
        [IncompleteReason::ResultsCapped { limit: 1 }]
    );
    assert_eq!(
        names(&humps),
        [("PremiumBilling".to_owned(), DeclKind::Class)]
    );
    query_golden::check(
        &goldens(),
        "sym.billing",
        &query_golden::symbols(&all).unwrap(),
    )
    .unwrap();
    query_golden::check(
        &goldens(),
        "sym.billing-capped",
        &query_golden::symbols(&capped).unwrap(),
    )
    .unwrap();
}

#[test]
fn should_build_the_name_tables_once_when_a_generation_is_queried_again() {
    // given
    let world = World::new();
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    let at = world.at("shop/Shop.java", "standard", 0, 2);
    let clock = still();
    let config = QueryConfig::default();
    let deadline = ClockDeadline::new(&clock, config.definition_budget);
    let cache = QueryCache::new();
    let run = |service: &Service, answer: &Answer| {
        block_on(definition(
            &pinned(answer),
            &service.sources(),
            at,
            UTF16,
            &config,
            &deadline,
            &cache,
        ))
        .unwrap()
    };
    // when
    let first = run(&service, &shop);
    let again = run(&service, &shop);
    service.finish(shop.id).unwrap();
    world
        .tree
        .write(
            &format!("m/{SRC}/shop/Extra.java"),
            b"package com.acme.shop;\n",
        )
        .unwrap();
    let after = ask(&mut service, "shop/Shop.java");
    let _ = run(&service, &after);
    // then
    assert_eq!(first, again);
    assert_eq!(cache.builds(), 2);
    assert!(after.generation.generation > shop.generation.generation);
}

/// One probe of the reviews (2026-11-19, two rounds): a site, and where javac 21 binds it (read
/// with `javap -c`, `-s` or `-v` from the compiled fixture). The oracle check: `definition` is
/// `exact` only on javac's declaration; until the syntax-tree resolver, a resolved name is never
/// `exact` at all (`queries::admit`).
struct Probe {
    name: &'static str,
    file: &'static str,
    needle: &'static str,
    nth: usize,
    skip: usize,
    javac: &'static str,
    /// javac's declaration when it is in the view (a symbol), else `None`.
    javac_in_view: Option<&'static str>,
}

/// The review's probes and two controls; javac's binding is the oracle: `exact` must name it,
/// and a binding syntax cannot prove is never `exact`.
const PROBES: [Probe; 26] = [
    Probe {
        name: "def.probe.annotation-key",
        file: "probe/Entity.java",
        needle: "name =",
        nth: 1,
        skip: 0,
        javac: "element Column.name() (RuntimeInvisibleAnnotations: Column(name=\"n\"))",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe.pattern-variable",
        file: "probe/Patterns.java",
        needle: "return count;",
        nth: 0,
        skip: 7,
        javac: "the pattern variable (aload_2), not the field",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe.field-beside-patterns",
        file: "probe/Patterns.java",
        needle: "return count;",
        nth: 1,
        skip: 7,
        javac: "getfield Patterns.count",
        javac_in_view: Some("com.acme.probe.Patterns#count"),
    },
    Probe {
        name: "def.probe.case-label",
        file: "probe/Painter.java",
        needle: "RED:",
        nth: 0,
        skip: 0,
        javac: "Color.RED (switch map Painter$1.$SwitchMap$com$acme$probe$Color)",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe.qualified-this",
        file: "probe/Outer.java",
        needle: "close();",
        nth: 0,
        skip: 0,
        javac: "invokevirtual com/acme/probe/Outer.close",
        javac_in_view: Some("com.acme.probe.Outer#close()"),
    },
    Probe {
        name: "def.probe.private-not-inherited",
        file: "probe/Host.java",
        needle: "helper();",
        nth: 0,
        skip: 0,
        javac: "invokevirtual com/acme/probe/Host.helper",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe.interface-static-not-inherited",
        file: "probe/Host2.java",
        needle: "tool();",
        nth: 0,
        skip: 0,
        javac: "invokestatic com/acme/probe/Host2.tool",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe.textual-override",
        file: "probe/SubDates.java",
        needle: "at(u)",
        nth: 0,
        skip: 0,
        javac: "invokevirtual at:(Ljava/util/Date;)V (SuperDates.at)",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe.enum-constant-body",
        file: "probe/Calc.java",
        needle: "return helper();",
        nth: 0,
        skip: 7,
        javac: "invokevirtual helper:()I of Calc$Op$1 (the constant's body)",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe.default-package-qualified",
        file: "probe/UsesEntry.java",
        needle: "Entry<",
        nth: 0,
        skip: 0,
        javac: "java/util/Map$Entry (field descriptor)",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe.inherited-across-packages",
        file: "shop/Discount.java",
        needle: "standard()",
        nth: 0,
        skip: 0,
        javac: "invokestatic standard (qualified by Discount, declared in Billing)",
        javac_in_view: Some("com.acme.billing.Billing#standard()"),
    },
    Probe {
        name: "def.probe.package-private-not-inherited",
        file: "shop/Retry.java",
        needle: "retries();",
        nth: 0,
        skip: 0,
        javac: "invokevirtual com/acme/shop/Retry.retries",
        javac_in_view: Some("com.acme.shop.Retry#retries()"),
    },
    Probe {
        name: "def.probe.outer-type",
        file: "probe/Outer.java",
        needle: "Outer.this",
        nth: 1,
        skip: 0,
        javac: "the enclosing class com.acme.probe.Outer",
        javac_in_view: Some("com.acme.probe.Outer"),
    },
    Probe {
        name: "def.probe2.anonymous-after-nested-new",
        file: "probe2/AnonNested.java",
        needle: "        helper();",
        nth: 0,
        skip: 8,
        javac: "AnonNested$1.run: invokevirtual helper:()I (the anonymous class's own)",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe2.anonymous-after-array-element",
        file: "probe2/AnonNested.java",
        needle: "        helper();",
        nth: 1,
        skip: 8,
        javac: "AnonNested$2.run: invokevirtual helper:()I (the anonymous class's own)",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe2.anonymous-after-lambda-block",
        file: "probe2/AnonNested.java",
        needle: "        helper();",
        nth: 2,
        skip: 8,
        javac: "AnonNested$3.run: invokevirtual helper:()I (the anonymous class's own)",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe2.element-key-after-comment",
        file: "probe2/Entity2.java",
        needle: "name = ",
        nth: 0,
        skip: 0,
        javac: "element Col2.name() (RuntimeInvisibleAnnotations: Col2(size=1, name=\"a\"))",
        javac_in_view: Some("com.acme.probe2.Col2#name()"),
    },
    Probe {
        name: "def.probe2.case-label-after-comment",
        file: "probe2/Painter2.java",
        needle: "GREEN:",
        nth: 0,
        skip: 0,
        javac: "Hue.GREEN (Painter2$1.$SwitchMap$com$acme$probe2$Hue)",
        javac_in_view: Some("com.acme.probe2.Hue#GREEN"),
    },
    Probe {
        name: "def.probe2.header-extends",
        file: "probe2/HSub.java",
        needle: "HBase {",
        nth: 0,
        skip: 0,
        javac: "super_class com/acme/probe2/HBase (the top-level type)",
        javac_in_view: Some("com.acme.probe2.HBase"),
    },
    Probe {
        name: "def.probe2.super-call",
        file: "probe2/HSub.java",
        needle: "super(1)",
        nth: 0,
        skip: 0,
        javac: "invokespecial com/acme/probe2/HBase.\"<init>\":(I)V",
        javac_in_view: Some("com.acme.probe2.HBase#<init>(int)"),
    },
    Probe {
        name: "def.probe2.header-annotation",
        file: "probe2/Tagged.java",
        needle: "@Marker\n",
        nth: 0,
        skip: 1,
        javac: "RuntimeInvisibleAnnotations: Lcom/acme/probe2/Marker; (the top-level type)",
        javac_in_view: Some("com.acme.probe2.Marker"),
    },
    Probe {
        name: "def.probe2.inherited-member-type-over-own-name",
        file: "probe2/Self.java",
        needle: "Self other",
        nth: 0,
        skip: 0,
        javac: "descriptor Lcom/acme/probe2/SelfBase$Self;",
        javac_in_view: Some("com.acme.probe2.SelfBase$Self"),
    },
    Probe {
        name: "def.probe2.type-parameter-after-annotation",
        file: "probe2/Box3.java",
        needle: "Money first",
        nth: 0,
        skip: 0,
        javac: "descriptor Ljava/lang/Object; (the type variable)",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe2.member-type-parameter-after-annotation",
        file: "probe2/Shelf.java",
        needle: "Row first",
        nth: 0,
        skip: 0,
        javac: "descriptor Ljava/lang/Object; (the type variable of Tray)",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe2.member-type-parameter-control",
        file: "probe2/Shelf.java",
        needle: "Row first",
        nth: 1,
        skip: 0,
        javac: "descriptor Ljava/lang/Object; (the type variable of Rack)",
        javac_in_view: None,
    },
    Probe {
        name: "def.probe2.anonymous-base-constructor",
        file: "probe2/AnonNested.java",
        needle: "new AnonBase(xs",
        nth: 0,
        skip: 4,
        javac: "invokespecial AnonNested$2.\"<init>\" calling AnonBase(int)",
        javac_in_view: Some("com.acme.probe2.AnonBase#<init>(int)"),
    },
];

#[test]
fn should_match_javac_or_stay_possible_when_the_review_s_probes_are_asked() {
    // given
    let world = World::new();
    let mut service = world.service();
    for probe in &PROBES {
        let answer = ask(&mut service, probe.file);
        let at = world.at(probe.file, probe.needle, probe.nth, probe.skip);
        // when
        let envelope = def(&service, &answer, at).unwrap_or_else(|e| panic!("{}: {e}", probe.name));
        // then: `exact` only on javac's declaration, and none of these sites is on the allowlist
        let exact = target(&envelope)
            .filter(|(_, status)| *status == TargetStatus::Exact && !envelope.exact().is_empty())
            .map(|(symbol, _)| symbol);
        assert!(
            exact.is_none() || exact.as_deref() == probe.javac_in_view,
            "{}: exact {exact:?}, javac binds {}",
            probe.name,
            probe.javac
        );
        assert_eq!(exact, None, "{}: javac binds {}", probe.name, probe.javac);
        assert!(!envelope.complete(), "{}", probe.name);
        golden_def(probe.name, probe.file, at, &envelope);
        service.finish(answer.id).unwrap();
    }
}

#[test]
fn should_answer_exact_only_on_a_declaration_s_own_name_when_any_name_is_asked() {
    // given: every declaration name and every reference site of the fixture project
    let world = World::new();
    let mut service = world.service();
    let files: Vec<String> = world
        .files
        .iter()
        .filter_map(|(p, _)| p.strip_prefix(&format!("{SRC}/")).map(str::to_owned))
        .collect();
    let mut sites = 0_usize;
    for file in &files {
        let answer = ask(&mut service, file);
        let Some(Some(jfl_application::Summary::Indexed { summary, .. })) = answer
            .generation
            .index
            .get(&answer.path)
            .map(|e| e.summary.clone())
        else {
            panic!("{file} is not indexed");
        };
        let text = world.text(file);
        let lines = LineIndex::new(&text).unwrap();
        let at = |offset| lines.position(offset, UTF16).unwrap();
        // when
        for (_, d) in summary.declarations() {
            let envelope = def(&service, &answer, at(d.name_range.start())).unwrap();
            // then
            assert_eq!(
                target(&envelope).map(|t| t.1),
                Some(TargetStatus::Exact),
                "{file}: a declaration's own name"
            );
        }
        for r in summary.references() {
            sites += 1;
            if let Ok(envelope) = def(&service, &answer, at(r.range.start())) {
                // then: no resolved name is `exact` (queries::admit)
                assert!(
                    envelope.exact().is_empty() && !envelope.complete(),
                    "{file}: {:?} answered {:?}",
                    summary.str(r.name),
                    envelope
                );
            }
        }
        service.finish(answer.id).unwrap();
    }
    assert!(sites > 150, "the corpus shrank: {sites} sites");
}

#[test]
fn should_say_incomplete_when_a_symbol_query_is_over_the_limit() {
    // given: a Java name may be longer than the 256 bytes the matcher takes
    let world = World::new();
    let mut service = world.service();
    let shop = ask(&mut service, "shop/Shop.java");
    let long = "A".repeat(jfl_application::queries::MAX_QUERY_BYTES + 1);
    let clock = still();
    let config = QueryConfig::default();
    // when
    let symbols = block_on(workspace_symbols(
        &pinned(&shop),
        &service.sources(),
        SymbolQuery {
            text: &long,
            filter: None,
            limit: None,
        },
        UTF16,
        &config,
        &ClockDeadline::new(&clock, config.symbols_budget),
        &QueryCache::new(),
    ));
    // then
    assert!(symbols.symbols.is_empty());
    assert_eq!(
        symbols.completeness.reasons(),
        [IncompleteReason::TargetUnresolved]
    );
}
