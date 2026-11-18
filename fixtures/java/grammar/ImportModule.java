package grammar;

// Java 25 (JEP 511): a module import. Upstream tree-sitter-java 0.23.5 cannot parse it.
import module java.base;

/** Uses a type the module import brings in. */
class ImportModule {
    List<String> names() {
        return List.of("a", "b");
    }
}
