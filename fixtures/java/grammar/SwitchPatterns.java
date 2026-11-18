package grammar;

/** Pattern matching: type patterns, record patterns and guards in a switch, and instanceof. */
class SwitchPatterns {
    record Pair(Object left, Object right) {}

    static String describe(Object o) {
        return switch (o) {
            case null -> "null";
            case Integer i when i > 10 -> "big " + i;
            case Integer i -> "int " + i;
            case Pair(String a, Integer b) -> a + b;
            case Pair(var a, var b) -> "pair " + a + b;
            case String s -> s.toUpperCase();
            default -> o.toString();
        };
    }

    static int length(Object o) {
        if (o instanceof String s && !s.isEmpty()) {
            return s.length();
        }
        if (!(o instanceof Pair(Object l, Object r))) {
            return 0;
        }
        return describe(l).length() + describe(r).length();
    }

    static int old(int day) {
        int result;
        switch (day) {
            case 1:
            case 7:
                result = 0;
                break;
            default:
                result = day;
        }
        return result;
    }
}
