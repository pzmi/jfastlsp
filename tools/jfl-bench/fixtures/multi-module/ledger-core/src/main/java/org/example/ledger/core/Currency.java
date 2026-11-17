package org.example.ledger.core;

/** The currencies a ledger can hold. */
public enum Currency {
    EUR,
    PLN,
    USD;

    /** The number of minor units in one major unit. */
    public int minorUnits() {
        return 100;
    }
}
