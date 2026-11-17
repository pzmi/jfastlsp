package org.example.ledger.core;

/** An amount in minor units of one currency. */
public record Money(long minor, Currency currency) {

    /** Zero in the given currency. */
    public static Money zero(Currency currency) {
        return new Money(0, currency);
    }

    /** The sum of this and another amount of the same currency. */
    public Money plus(Money other) {
        requireSameCurrency(other);
        return new Money(Math.addExact(minor, other.minor), currency);
    }

    /** This amount negated. */
    public Money negate() {
        return new Money(Math.negateExact(minor), currency);
    }

    private void requireSameCurrency(Money other) {
        if (other.currency != currency) {
            throw new IllegalArgumentException("currency mismatch");
        }
    }
}
