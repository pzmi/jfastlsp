package syntax;

import static java.util.Objects.requireNonNull;
import java.util.*;
import java.util.function.Function;

/** Reference sites by shape; comments and strings hold none. */
class References extends Base {
    static int counter;
    String name;

    References() {
        this("default");
    }

    References(String name) {
        super(name, 1);
        this.name = requireNonNull(name);
    }

    // charge(money) in a line comment is not a reference
    /* nor charge(money) in a block comment */
    void calls(List<String> items, Base other) {
        String text = "charge(money) inside a string";
        char c = 'x';
        helper(1, /* not an argument */ 2);
        other.helper(3, 4);
        items.forEach(System.out::println);
        Function<String, References> make = References::new;
        Function<String, Integer> len = String::length;
        int total = References.counter + other.field + Base.CONSTANT;
        new ArrayList<String>(items).add(text + c);
        make.apply(name);
        len.apply(text);
        helper(total, items.size());
    }

    @Deprecated
    @SuppressWarnings({"unused", "rawtypes"})
    void annotated() {
        outer:
        for (String s : List.of("a")) {
            if (s.isEmpty()) {
                continue outer;
            }
            break outer;
        }
    }
}

class Base {
    static final int CONSTANT = 1;
    int field;

    Base(String name, int count) {}

    int helper(int a, int b) {
        return a + b;
    }
}
