# Lombok corpus spike (2026-11-17)

Method: `git ls-remote` + `git clone --depth 1` (no Gradle, no build); counts via `git ls-files` / `git grep`;
parse via the e1-parse harness (tree-sitter 0.27.2, java-orchard grammar), 4 vCPU VM, cold = after drop_caches,
warm = median of 3, peak RSS from GNU time. Candidate survey cloned (blob-less) then deleted; kept: repos/fineract, repos/rewrite.

## Candidates
| Repo | Gradle wrapper | .java | files importing lombok | Processors | Licence | Verdict |
|:--|:--|--:|--:|:--|:--|:--|
| apache/fineract | yes (39 includes) | 7,301 | 2,874 | Lombok (29 modules), MapStruct (24 modules, 123 files), Spring config/autoconfigure processors | Apache-2.0 | top |
| openrewrite/rewrite | yes (~40 modules) | 4,577 | 1,431 | Lombok (own javac-handler module too), picocli-codegen; MapStruct barely (5 files) | Apache-2.0 core + Moderne Source Available (MSAL) modules (android, csharp, go, ...) | runner-up (licence mixed) |
| datahub-project/datahub | yes | 5,656 | 1,805 | Lombok; no MapStruct | Apache-2.0 | alternative; repo is mixed Java/TS/Python, not parsed |
| apache/gravitino | yes | 5,417 | 371 | Lombok | Apache-2.0 | light Lombok use |
| Netflix/conductor | yes | 516 | 0 | none (last commit 2023, archived) | Apache-2.0 | reject |
| apache/polaris | yes | 1,691 | 0 | Immutables | Apache-2.0 | no Lombok |
| thingsboard, openapi-generator, OpenTripPlanner | Maven | - | - | - | - | excluded (not Gradle); not cloned |

## Detail of the top two
| | fineract | rewrite |
|:--|:--|:--|
| SHA | 9dfe8dfe5e162afe09f102a9aa57d00a70c770f2 (2026-11-17) | 20d66ca842cd62ac9be6def51e85ffba2c7275f2 (2026-11-17) |
| .java files / MB / lines | 7,301 / 50.5 / 968,738 | 4,577 / 45.7 / 1,183,471 |
| test files | 1,728 java under src/test | 1,641 under src/test |
| Lombok annotations (files) | RequiredArgsConstructor 1757, @Slf4j 673 (lombok.extern.slf), Getter 577, NoArgsConstructor 445, Data 403, AllArgsConstructor 360, Setter 249, Builder 205, Accessors 162, EqualsAndHashCode 71, FieldNameConstants 25, ToString 20, Jacksonized 3, SneakyThrows 5 | Value 1020, EqualsAndHashCode 470, With 362, Getter 301, RequiredArgsConstructor 123, Builder 57, FieldDefaults 56, NonFinal 43, NonNull 34 |
| lombok.config | 1 (root; addLombokGeneratedAnnotation, copyableAnnotations, jacksonized) | 1 (root; copyableAnnotations, addNullAnnotations, val/var flagUsage=error) |
| Generated/delombok sources committed | none (fineract-client is OpenAPI-generated at build time; not in tree) | none |
| Mapper classes | 179 *Mapper*.java | 2 |

## Parse (orchard grammar, harness E1)
| | Warm 4 thr | Cold 4 thr | Files/s warm | p95/file | max/file | RSS | 1 thr warm | Error files |
|:--|--:|--:|--:|--:|--:|--:|--:|--:|
| fineract | 1.07 s | 1.35 s | ~7,160 | 2.0 ms | 71 ms | 58 MB | 3.85 s | 0 / 7,301 (0.00 %) |
| rewrite | 0.96 s | 1.21 s | ~4,890 | 3.0 ms | 34-67 ms | 35 MB | 3.89 s | 0 / 4,577 (0.00 %) |
Upstream grammar: fineract 0 errors, rewrite 1 file (0.02 %). Lombok syntax is plain annotations, so parsing is unaffected;
declarations seen by a syntax-only index lack the generated members (getters, builders, constructors, MapStruct impls) - that is the SEM/javac-worker problem, not parse.

## Recommendation
- (a) Corpus slot `lombok` (medium, Apache-2.0, fits the existing medium cost ~1 s): **apache/fineract** at the SHA above. Permissive, Gradle, heaviest and most diverse Lombok use (constructor/builder/accessor/log/Jacksonized), plus MapStruct in 24 modules, multi-module, active daily, real tests.
- (b) Second E0/E3 target covering processors: **fineract** too. Runner-up **openrewrite/rewrite**: different Lombok idiom (@Value/@With/@FieldDefaults, immutable-style), but mixed licence (MSAL modules - keep timings-only or restrict to Apache paths, like elasticsearch), multi-release java-8..25 source sets, non-Java modules, and almost no MapStruct. datahub is a third option if more Lombok volume is wanted (mixed-language tree).
- ADR 0032: spring-framework stays primary E0 target (pure Java, no processors); fineract is added as a second target specifically for Lombok/MapStruct generated-member answers (completeness must degrade honestly when generated sources are absent). No reversal of 0032; an amendment adding the second target.
- Caveats: counts at depth-1 HEAD only (history not inspected beyond that HEAD being current); build-time generated dirs not verified without a Gradle run; fineract needs its own pin in scripts/bench-corpus.sh and doc/bench-corpus.md (counts: 7,301 files / 50.5 MB / 968,738 lines).
