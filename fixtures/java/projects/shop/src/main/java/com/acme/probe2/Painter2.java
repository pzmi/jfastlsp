package com.acme.probe2;

/** A `case` label after a comment. */
class Painter2 {
    static final int GREEN = 2;

    int paint(Hue h) {
        switch (h) {
            case RED, /* the other one */ GREEN:
                return 1;
            default:
                return 0;
        }
    }
}
