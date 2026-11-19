package com.acme.probe;

/** A `case` label names the selector's enum constant, not this class's field of that name. */
class Painter {
    static final int RED = 1;

    int paint(Color c) {
        switch (c) {
            case RED:
                return 1;
            default:
                return 0;
        }
    }
}
