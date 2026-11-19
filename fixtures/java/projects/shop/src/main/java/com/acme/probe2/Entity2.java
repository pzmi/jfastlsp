package com.acme.probe2;

/** An element key after a comment. */
class Entity2 {
    String name;

    @Col2(size = 1, /* the column */ name = "a")
    String a;
}
