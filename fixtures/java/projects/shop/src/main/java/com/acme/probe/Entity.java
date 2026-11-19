package com.acme.probe;

/** `name = "n"` is the annotation's element, not this class's field. */
class Entity {
    @Column(name = "n")
    String name;
}
