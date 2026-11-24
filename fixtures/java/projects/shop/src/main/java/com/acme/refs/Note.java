package com.acme.refs;

/** A note on a declaration: its value is often set without its name. */
@interface Note {
    String value();

    int weight() default 1;
}
