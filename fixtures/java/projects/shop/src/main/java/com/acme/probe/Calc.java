package com.acme.probe;

/** An enum constant's body is an anonymous class: `helper()` there is the body's own. */
class Calc {
    static int helper() {
        return 0;
    }

    enum Op {
        PLUS {
            int helper() {
                return 1;
            }

            @Override
            int apply() {
                return helper();
            }
        };

        abstract int apply();
    }
}
