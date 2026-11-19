package com.acme.shop;

import com.acme.money.Money;

/** A cart; its charge is not Billing's. */
public class Cart {
    private Money total = new Money(0);

    /** Adds {@code price} to the total. */
    public void add(Money price) {
        total = total.plus(price);
    }

    /** The cart's own charge: another type's member of Billing's name. */
    public Money charge(Money amount) {
        return amount;
    }

    /** The total so far. */
    public Money total() {
        return total;
    }

    <T> T pick(T first, T second) {
        return first;
    }
}
