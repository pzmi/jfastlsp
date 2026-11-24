package com.acme.refs;

/** Keeps a running total. The word post is here: post(1) in Javadoc is no call. */
public class Tally {
    private int count;
    int total;

    /** Makes an empty tally. */
    public Tally() {}

    Tally(int start) {
        this();
        total = start;
    }

    /** Adds one amount; see post(int, int). */
    public void post(int amount) {
        // post(amount) twice would double it
        total += amount;
        count++;
    }

    /** Adds two amounts. */
    public void post(int first, int second) {
        post(first);
        this.post(second);
    }

    /* A static helper: twice(9) here is in a comment. */
    static int twice(int value) {
        return value * 2;
    }

    int count() {
        return count;
    }

    /** Adds every amount. */
    void postAll(int... amounts) {
        for (int amount : amounts) {
            post(amount);
        }
    }
}
