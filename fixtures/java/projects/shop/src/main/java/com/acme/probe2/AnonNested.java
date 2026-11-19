package com.acme.probe2;

/** Anonymous classes whose arguments hold a nested `new`, an array element or a lambda block. */
class AnonNested {
    int helper() {
        return 1;
    }

    void nestedNew() {
        AnonBase a = new AnonBase(new Object()) {
            int helper() {
                return 2;
            }

            @Override
            void run() {
                helper();
            }
        };
    }

    void arrayElement(int[] xs) {
        AnonBase a = new AnonBase(xs[0]) {
            int helper() {
                return 3;
            }

            @Override
            void run() {
                helper();
            }
        };
    }

    void lambdaBlock() {
        AnonBase a = new AnonBase(() -> { }) {
            int helper() {
                return 4;
            }

            @Override
            void run() {
                helper();
            }
        };
    }
}
