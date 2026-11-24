package com.acme.refs;

/** An entry: its accessors are its components' implicit methods. */
record Entry(Kind kind, int amount) {
    int signed() {
        return kind.sign * amount();
    }

    static Entry debit(int amount) {
        return new Entry(Kind.DEBIT, amount);
    }
}
