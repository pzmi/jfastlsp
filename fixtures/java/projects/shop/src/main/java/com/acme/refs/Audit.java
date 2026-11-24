package com.acme.refs;

/** A tally that starts at five. */
class Audit extends Tally {
    Audit() {
        super(5);
    }
}

/** A tally made with the implicit call of its superclass's constructor. */
class Spot extends Tally {}

/** Posts amounts in batches, with as many arguments as it likes. */
class Batch {
    void run(Tally tally) {
        tally.postAll();
        tally.postAll(1, 2, 3);
    }
}
