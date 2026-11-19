package com.acme.probe2;

/** The header and `super(...)` see the top-level HBase, not the member type of the same name. */
class HSub extends HBase {
    static class HBase {
        HBase(int x) {}
    }

    HSub() {
        super(1);
    }
}
