package syntax;

// Stored with LF; tests/goldens.rs turns every line end into CRLF in memory and checks the result
// against summaries/orchard-crlf/: a line comment then ends before its \r.
/** Javadoc on CRLF lines. */
class LineEnds { // trailing comment
    int field; // another

    void method() {
        field++; // last
    }
}
