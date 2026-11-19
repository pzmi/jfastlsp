package com.acme.probe;

/** `helper()` in Sub is Host's: Base's is private, so Sub does not inherit it. */
class Host {
    void helper() {}

    class Sub extends Base {
        void f() {
            helper();
        }
    }
}
