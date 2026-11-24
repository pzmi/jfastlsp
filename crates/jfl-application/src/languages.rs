//! The language registry (IDX-17): the one place that says what language a file is, by its name
//! alone (no read, no parse).
//!
//! - **Indexed languages** ([`INDEXED_LANGUAGES`]): each has a [`Language`] id and its
//!   extensions. A listing lists their files, the git adapter asks `ls-files` for them, and the
//!   view service hands each file to the indexer composition wired for its language
//!   ([`crate::syntax_indexer::SyntaxIndexers`]); every summary records the language
//!   ([`jfl_domain::FileSummary::language`]).
//! - **Other JVM languages and build scripts** (IDX-16: [`OTHER_JVM_EXTENSIONS`],
//!   [`BUILD_SCRIPT_SUFFIXES`]): not indexed, counted, so an answer they may contradict says so.
//!
//! **What adding a language takes** (Go, Kotlin, Python): a [`Language`] variant in `jfl-domain`
//! (every exhaustive `match` then says what else must decide), a row here with its extensions
//! (taken out of [`OTHER_JVM_EXTENSIONS`] if it was counted there, as Kotlin is), an adapter that
//! implements [`crate::syntax_indexer::SyntaxIndexer`] for it, and composition's wiring
//! (`jfl-composition`'s `languages` module, whose test fails while a registered language has no
//! indexer). Resolution rules per language are the second slice of IDX-17 (`doc/tasks.md`).
//!
//! **A static table, not one injected at run time.** The listing (the fs adapter's walk and the
//! testkit fake), the git adapter's pathspecs, the hint filter and the view service each classify
//! names without a port between them; one `const` table keeps them in agreement by construction,
//! and the contract suites check each implementation against it. Rejected alternative: a
//! registry built by composition and passed to every adapter, which would let two adapters be
//! built with different registries and disagree about what a view holds. Injection is worth it
//! only once a language can be switched off at run time, which nothing asks for.

use jfl_domain::Language;

/// An indexed language and the file extensions that are its sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexedLanguage {
    /// The language.
    pub language: Language,
    /// Its extensions, with their dot, matched case-sensitively as the language's own compiler
    /// does (`javac` reads `.java` and refuses `.JAVA`).
    pub extensions: &'static [&'static str],
}

/// Every indexed language, in [`Language`] code order, with its extensions; no extension belongs to
/// two languages, nor to [`OTHER_JVM_EXTENSIONS`] or [`BUILD_SCRIPT_SUFFIXES`].
///
/// Pinned by: `languages::tests::should_give_each_extension_one_rule_when_the_registry_is_read`.
pub const INDEXED_LANGUAGES: [IndexedLanguage; 1] = [IndexedLanguage {
    language: Language::Java,
    extensions: &[".java"],
}];

/// The indexed language of a file of this name, by its bytes (a name that is not UTF-8 still
/// has an extension, and a listing reports such a file as a gap of its language); `None` when
/// no indexed language reads it. A name must hold more than the extension.
///
/// Pinned by: `languages::tests::should_classify_a_file_by_its_extension_when_a_listing_meets_it`.
#[must_use]
pub fn indexed_language_of_bytes(name: &[u8]) -> Option<Language> {
    INDEXED_LANGUAGES
        .iter()
        .find(|indexed| {
            indexed.extensions.iter().any(|extension| {
                name.len() > extension.len() && name.ends_with(extension.as_bytes())
            })
        })
        .map(|indexed| indexed.language)
}

/// [`indexed_language_of_bytes`] of a UTF-8 name.
#[must_use]
pub fn indexed_language(name: &str) -> Option<Language> {
    indexed_language_of_bytes(name.as_bytes())
}

/// Whether a file of this name is listed and indexed: it has an indexed language's extension.
#[must_use]
pub fn is_indexed_file_name(name: &str) -> bool {
    indexed_language(name).is_some()
}

/// A JVM language other than Java whose sources can name Java declarations: not indexed in v1
/// (plan §1.4), so a view holding one cannot prove an answer whole (IDX-16, ADR 0014).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OtherLanguage {
    /// Kotlin/JVM.
    Kotlin,
    /// Groovy.
    Groovy,
    /// Scala.
    Scala,
    /// Clojure on the JVM.
    Clojure,
    /// Xtend.
    Xtend,
    /// JavaServer Pages and JSP tag files.
    Jsp,
    /// AspectJ: the one language here whose files can change what a Java name is proved bound
    /// to (inter-type declarations add members, `declare parents` supertypes).
    AspectJ,
    /// BeanShell.
    BeanShell,
    /// Groovy Server Pages (Grails).
    Gsp,
    /// jte templates (`.jte` compiles to Java, `.kte` to Kotlin).
    Jte,
}

/// The allowlist of other JVM languages' file extensions, matched without regard to ASCII case
/// (IDX-16). Each is here because files of that extension name Java declarations directly and
/// are compiled or run on the same classpath as the view's Java:
/// - `.kt`: Kotlin calls Java as its own code; Gradle's Kotlin plugin compiles a source set's
///   Kotlin and Java together, so each sees the other's declarations.
/// - `.kts`: Kotlin scripts other than Gradle's (those are [`BUILD_SCRIPT_SUFFIXES`]): a script's
///   classpath may hold the project's classes.
/// - `.groovy`, `.gvy`, `.gy`, `.gsh`: Groovy sources and scripts (`groovyc` and `groovysh` take
///   all four): Spock specifications under `src/test/groovy`, Groovy beans and the scripts a
///   framework loads at run time (spring-framework's test resources hold some).
/// - `.scala`, `.sc`: Scala sources, and Scala scripts and worksheets (Ammonite, scala-cli, Mill's
///   `build.sc`).
/// - `.clj`, `.cljc`: Clojure on the JVM names Java classes and members by interop; `.cljc`
///   files compile for the JVM too.
/// - `.xtend`: Xtend compiles to Java and calls it.
/// - `.jsp`, `.jspf`, `.jspx`: JSP pages and fragments compile to servlets; their imports and
///   scriptlets are Java. `.tag`, `.tagx`: JSP tag files, compiled the same way.
/// - `.aj`: AspectJ is compiled by `ajc` together with the Java it advises and calls it; its
///   inter-type declarations and `declare parents` also add members and supertypes to Java
///   classes, so even a proved binding can be wrong while one is in the view
///   ([`OtherSources::aspects_seen_from`](crate::source_tree::OtherSources::aspects_seen_from); spring-framework's `spring-aspects` holds some).
/// - `.bsh`: BeanShell scripts implement Java interfaces and call Java (spring-framework's
///   scripting tests load some).
/// - `.gsp`: Groovy Server Pages compile to Groovy classes whose imports and scriptlets call Java.
/// - `.jte`, `.kte`: jte templates compile to Java or Kotlin classes; their `@import` and
///   `@param` lines and expressions name Java declarations (junit-framework's build holds some).
///
/// Left out, each on purpose: `.cljs` (ClojureScript compiles to JavaScript); `.py`, `.rb`, `.js`
/// (Jython, JRuby and the JVM's JavaScript engines can call Java, but such files are almost
/// always CPython, Ruby or browser and Node code, and counting them would withdraw `complete`
/// from every repository with a tool script); `.xml`, `.properties`, `META-INF/services` and
/// other text that names a class in a string (a reflective use, which no language's syntax binds);
/// `.class` and `.jar` (compiled libraries: PROJ-03); templates that reach Java only through
/// reflective property paths (Thymeleaf and FreeMarker `.html`/`.ftl`, Velocity `.vm`,
/// Mustache), the same case as a string.
///
/// Pinned by: `languages::tests::should_classify_a_file_by_its_extension_when_a_listing_meets_it`.
pub const OTHER_JVM_EXTENSIONS: [(&str, OtherLanguage); 21] = [
    (".kt", OtherLanguage::Kotlin),
    (".kts", OtherLanguage::Kotlin),
    (".groovy", OtherLanguage::Groovy),
    (".gvy", OtherLanguage::Groovy),
    (".gy", OtherLanguage::Groovy),
    (".gsh", OtherLanguage::Groovy),
    (".scala", OtherLanguage::Scala),
    (".sc", OtherLanguage::Scala),
    (".clj", OtherLanguage::Clojure),
    (".cljc", OtherLanguage::Clojure),
    (".xtend", OtherLanguage::Xtend),
    (".jsp", OtherLanguage::Jsp),
    (".jspf", OtherLanguage::Jsp),
    (".jspx", OtherLanguage::Jsp),
    (".tag", OtherLanguage::Jsp),
    (".tagx", OtherLanguage::Jsp),
    (".aj", OtherLanguage::AspectJ),
    (".bsh", OtherLanguage::BeanShell),
    (".gsp", OtherLanguage::Gsp),
    (".jte", OtherLanguage::Jte),
    (".kte", OtherLanguage::Jte),
];

/// Gradle build scripts, Groovy (`.gradle`) and Kotlin (`.gradle.kts`) DSL: `build.gradle`,
/// `settings.gradle.kts`, a script plugin applied with `apply from:`, an init script.
///
/// A build script is compiled against the build's own classpath: Gradle's API, its plugins, its
/// `buildscript` dependencies, and the classes of `buildSrc` and of included builds; as a rule not
/// against the source sets of the build it configures, which that build compiles after its
/// scripts ran. So a script counts only for a target in build logic
/// ([`OtherSources::in_build_logic`](crate::source_tree::OtherSources::in_build_logic)), by directory. **A known limitation until the project
/// model (M3):** build logic can compile sources from elsewhere (`buildSrc` with
/// `srcDir("../src/main/java")`, an included build whose source directories lie outside it,
/// `buildscript { classpath files(...) }` naming a project's output), and an included build
/// without a settings file of its own (which Gradle 8 refuses) is not recognised as one; a
/// script's use of such a declaration is then not counted.
pub const BUILD_SCRIPT_SUFFIXES: [&str; 2] = [".gradle", ".gradle.kts"];

/// What a file is to a listing, by its name alone (IDX-16): no read, no parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    /// A file of an indexed language ([`INDEXED_LANGUAGES`]): listed and summarised by that
    /// language's indexer.
    Indexed(Language),
    /// A source file of another JVM language ([`OTHER_JVM_EXTENSIONS`]): counted.
    Other(OtherLanguage),
    /// A Gradle build script ([`BUILD_SCRIPT_SUFFIXES`]): counted for build logic only.
    BuildScript,
}

/// What a file of this name is, `None` when no listing rule reads it. A name must hold more than
/// its extension (`.kt` alone is no Kotlin file, as `.java` alone is no Java file). An indexed
/// language's extension wins over every other rule.
///
/// Pinned by: `languages::tests::should_classify_a_file_by_its_extension_when_a_listing_meets_it`.
#[must_use]
pub fn source_kind(name: &str) -> Option<SourceKind> {
    if let Some(language) = indexed_language(name) {
        return Some(SourceKind::Indexed(language));
    }
    let has = |suffix: &str| {
        name.len()
            .checked_sub(suffix.len())
            .filter(|at| *at > 0)
            .and_then(|at| name.get(at..))
            .is_some_and(|end| end.eq_ignore_ascii_case(suffix))
    };
    if BUILD_SCRIPT_SUFFIXES.iter().any(|suffix| has(suffix)) {
        return Some(SourceKind::BuildScript);
    }
    OTHER_JVM_EXTENSIONS
        .iter()
        .find(|(extension, _)| has(extension))
        .map(|(_, language)| SourceKind::Other(*language))
}

/// Whether a listing counts a file of this name as another JVM language's source or a build
/// script ([`source_kind`]).
#[must_use]
pub fn is_other_source_name(name: &str) -> bool {
    matches!(
        source_kind(name),
        Some(SourceKind::Other(_) | SourceKind::BuildScript)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_classify_a_file_by_its_extension_when_a_listing_meets_it() {
        // given
        let named = [
            ("A.java", Some(SourceKind::Indexed(Language::Java))),
            ("Ledger.kt", Some(SourceKind::Other(OtherLanguage::Kotlin))),
            (
                "tool.main.kts",
                Some(SourceKind::Other(OtherLanguage::Kotlin)),
            ),
            (
                "Spec.groovy",
                Some(SourceKind::Other(OtherLanguage::Groovy)),
            ),
            ("run.gsh", Some(SourceKind::Other(OtherLanguage::Groovy))),
            ("Main.scala", Some(SourceKind::Other(OtherLanguage::Scala))),
            ("build.sc", Some(SourceKind::Other(OtherLanguage::Scala))),
            ("core.cljc", Some(SourceKind::Other(OtherLanguage::Clojure))),
            ("Gen.xtend", Some(SourceKind::Other(OtherLanguage::Xtend))),
            ("index.jsp", Some(SourceKind::Other(OtherLanguage::Jsp))),
            ("field.tagx", Some(SourceKind::Other(OtherLanguage::Jsp))),
            ("Audit.aj", Some(SourceKind::Other(OtherLanguage::AspectJ))),
            (
                "Calc.bsh",
                Some(SourceKind::Other(OtherLanguage::BeanShell)),
            ),
            ("list.gsp", Some(SourceKind::Other(OtherLanguage::Gsp))),
            ("page.jte", Some(SourceKind::Other(OtherLanguage::Jte))),
            ("page.kte", Some(SourceKind::Other(OtherLanguage::Jte))),
            ("page.ftl", None),
            ("page.vm", None),
            ("Shout.KT", Some(SourceKind::Other(OtherLanguage::Kotlin))),
            ("build.gradle", Some(SourceKind::BuildScript)),
            ("settings.gradle.kts", Some(SourceKind::BuildScript)),
            ("conventions.gradle", Some(SourceKind::BuildScript)),
            (".kt", None),
            (".gradle", None),
            ("app.cljs", None),
            ("tool.py", None),
            ("beans.xml", None),
            ("A.class", None),
            ("A.javax", None),
            ("Makefile", None),
            ("é.kt", Some(SourceKind::Other(OtherLanguage::Kotlin))),
        ];
        // when
        let kinds: Vec<Option<SourceKind>> = named.iter().map(|(n, _)| source_kind(n)).collect();
        // then
        let want: Vec<Option<SourceKind>> = named.iter().map(|(_, k)| *k).collect();
        assert_eq!(kinds, want);
        assert!(OTHER_JVM_EXTENSIONS.iter().all(|(e, _)| e.starts_with('.')));
    }

    #[test]
    fn should_give_each_extension_one_rule_when_the_registry_is_read() {
        // given
        let indexed = INDEXED_LANGUAGES
            .iter()
            .flat_map(|indexed| indexed.extensions.iter().copied());
        let counted = OTHER_JVM_EXTENSIONS
            .iter()
            .map(|(extension, _)| *extension)
            .chain(BUILD_SCRIPT_SUFFIXES);
        // when
        let mut all: Vec<String> = indexed
            .chain(counted)
            .map(str::to_ascii_lowercase)
            .collect();
        let before = all.len();
        all.sort_unstable();
        all.dedup();
        // then
        assert_eq!(all.len(), before, "an extension belongs to two rules");
        let codes: Vec<u8> = INDEXED_LANGUAGES
            .iter()
            .map(|i| i.language.code())
            .collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            codes, sorted,
            "languages out of code order, or one listed twice"
        );
        assert!(
            INDEXED_LANGUAGES.iter().all(
                |i| !i.extensions.is_empty() && i.extensions.iter().all(|e| e.starts_with('.'))
            )
        );
    }
}
