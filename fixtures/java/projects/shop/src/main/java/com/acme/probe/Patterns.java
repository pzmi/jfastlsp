package com.acme.probe;

import java.util.List;

/** Bare names bound by patterns, lambdas, catch, for and try: never this class's fields. */
class Patterns {
    int count;
    String text;
    Object item;

    int pattern(Object o) {
        if (o instanceof Integer count) {
            return count;
        }
        return 0;
    }

    int lambda(List<String> values) {
        values.forEach(text -> text.length());
        return values.size();
    }

    int caught() {
        try {
            return 1;
        } catch (RuntimeException item) {
            return item.hashCode();
        }
    }

    int loop(List<Object> values) {
        int n = 0;
        for (Object item : values) {
            n += item.hashCode();
        }
        return n;
    }

    int field() {
        return count;
    }
}
