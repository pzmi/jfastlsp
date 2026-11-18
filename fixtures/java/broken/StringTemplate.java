// This file exists to fail: a string template (preview in Java 21 and 22, withdrawn in 23) is not
// Java, so it is a syntax error even where a grammar parses it (ADR 0022).
package broken;

class StringTemplate {
    String greet(String name) {
        return STR."Hello \{name}!";
    }
}
