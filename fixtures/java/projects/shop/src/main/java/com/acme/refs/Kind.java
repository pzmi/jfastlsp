package com.acme.refs;

/** Kinds of entry, each made by the enum's constructor without its name. */
enum Kind {
    DEBIT(1),
    CREDIT(-1);

    final int sign;

    Kind(int sign) {
        this.sign = sign;
    }
}
