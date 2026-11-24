package com.acme.refs;

import java.util.function.IntConsumer;
import java.util.function.IntUnaryOperator;
import java.util.function.Supplier;

/** Uses a tally: "post" in text is no reference. */
@Note("clerk")
class Clerk {
    private final Tally tally = new Tally(1);

    @Note(value = "work", weight = 2)
    void work(int amount) {
        tally.post(amount);
        tally.post(amount, amount);
        String note = "tally.post(amount) is written in a string";
        char p = 'p';
        String block = """
            post(1); Tally.twice(2); new Tally(); total
            """;
        // tally.post(3) in a line comment
        /* tally.post(4) in a block comment, and tally.total */
        IntConsumer each = tally::post;
        IntUnaryOperator twice = Tally::twice;
        Supplier<Tally> fresh = Tally::new;
        each.accept(twice.applyAsInt(note.length() + block.length() + p));
        tally.total = tally.count() + fresh.get().total;
    }
}
