# The golden Java corpus

Synthetic Java (ADR 0004: no code from any real project) and the summaries the syntax index must
produce for it (ver §4, IDX-07). Every file here is locked in `fixtures.lock`
(`ARCH-fixtures-lock`); `just fixtures-lock` rewrites the lock after a reviewed change.

| Directory | Holds |
|:--|:--|
| `grammar/` | One small file per language feature, parsed by both grammars of ADR 0022: records, sealed types, switch patterns, text blocks, `import module` (Java 25), flexible constructor bodies (Java 25), annotations in every position |
| `syntax/` | One file per extraction rule: declarations and erasure, reference shapes (comments and strings hold none), locals, Javadoc, Unicode escapes and ignorable characters, CRLF line ends (`-text` in `.gitattributes`) |
| `broken/` | Files that exist to fail, each saying so in a header comment: syntax errors, a string template |
| `summaries/orchard/` | The expected summary of every file above with tree-sitter-java-orchard |
| `summaries/upstream/` | The expected summary of every `grammar/` file with upstream tree-sitter-java, and `known-gaps.txt`, the files it reports syntax errors for |

A summary golden is never rewritten to get green. A new corpus file fails
`crates/jfl-adapter-java/tests/goldens.rs` once, which writes its golden for review; a deliberate
change deletes the golden, reruns, reviews the new file, and moves its `fixtures.lock` line.

`ImportModule.java` and `FlexibleConstructorBodies.java` need Java 25; the rest compile with the
pinned JDK 21. A byte-order mark is tested in memory (`javac` refuses one in a file).
