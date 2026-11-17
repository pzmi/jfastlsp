package org.example.ledger.core;

import java.util.Objects;

/** A named account with a running balance. */
public final class Account {
    private final String id;
    private Money balance;

    public Account(String id, Currency currency) {
        this.id = Objects.requireNonNull(id);
        this.balance = Money.zero(currency);
    }

    public String id() {
        return id;
    }

    public Money balance() {
        return balance;
    }

    void apply(Entry entry) {
        balance = balance.plus(entry.amount());
    }
}
