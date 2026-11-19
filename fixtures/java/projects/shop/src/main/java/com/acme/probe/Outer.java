package com.acme.probe;

/** `Outer.this.close()` calls Outer's method, not Inner's. */
class Outer {
    void close() {}

    class Inner {
        void close() {
            Outer.this.close();
        }
    }
}
