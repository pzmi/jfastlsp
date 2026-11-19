package com.acme.probe;

/** `tool()` in Impl is Host2's: Util's is a static interface method. */
class Host2 {
    static void tool() {}

    class Impl implements Util {
        void f() {
            tool();
        }
    }
}
