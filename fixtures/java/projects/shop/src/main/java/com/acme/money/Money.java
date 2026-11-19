package com.acme.money;

/** An amount of money in minor units. */
public final class Money {
    private final long cents;

    /** Money of {@code cents} minor units. */
    public Money(long cents) {
        this.cents = cents;
    }

    /** Money of whole units and minor units. */
    public Money(int units, int cents) {
        this(units * 100L + cents);
    }

    /** The amount in minor units. */
    public long cents() {
        return cents;
    }

    /** This amount and {@code other} together. */
    public Money plus(Money other) {
        return new Money(cents + other.cents);
    }
}
