package com.acme.probe2;

/** A base for anonymous classes, with constructors taking an object, an int and a runnable. */
abstract class AnonBase {
    AnonBase(Object o) {}

    AnonBase(int i) {}

    AnonBase(Runnable r) {}

    abstract void run();
}
