package grammar;

/** Text blocks: names inside them are text, never references. */
class TextBlocks {
    static final String QUERY = """
        SELECT name FROM Account WHERE owner = ?
        AND balance > 0 \
        ORDER BY name""";

    static final String JSON = """
        { "method": "charge", "escaped": "\"quoted\"" }
        """;

    static String render(String name) {
        return """
            Hello, %s!
            """.formatted(name);
    }
}
