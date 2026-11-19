package com.acme.shop;

import com.acme.money.Money;
import java.util.ArrayList;

/** A ledger is a list of amounts: its supertype is outside the index. */
public class Ledger extends ArrayList<Money> {
    Money sum() {
        Money total = new Money(0);
        for (Money amount : this) {
            total = total.plus(amount);
        }
        return total;
    }

    void record(Money amount) {
        add(amount);
    }

    Money first() {
        return sum();
    }
}
