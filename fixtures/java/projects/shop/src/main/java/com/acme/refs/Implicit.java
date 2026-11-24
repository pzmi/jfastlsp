package com.acme.refs;

import java.lang.annotation.ElementType;
import java.lang.annotation.Target;

/** Constructors and accessors called without their names. */
class Implicit {
    /** A type-use annotation, on a superclass below. */
    @Target(ElementType.TYPE_USE)
    @interface Mark {}

    /** Its constructor is called by `super(…)` on an annotated superclass. */
    static class Marked {
        Marked(int start) {}
    }

    static class MarkedSub extends @Mark Marked {
        MarkedSub() {
            super(1);
        }
    }

    /** Its variable-arity constructor is called by its subclass's implicit `super()`. */
    static class Spread {
        Spread(int... xs) {}
    }

    static class SpreadSub extends Spread {}

    /** Its private constructor is called by a nested subclass's implicit `super()`. */
    static class Secret {
        private Secret() {}
    }

    static class SecretSub extends Secret {}

    /** A point: a record pattern calls its accessors. */
    record Pt(int x, int y) {
        public int y() {
            return y;
        }
    }

    static int sum(Object o) {
        if (o instanceof Pt(int a, int b)) {
            return a + b;
        }
        return 0;
    }
}
