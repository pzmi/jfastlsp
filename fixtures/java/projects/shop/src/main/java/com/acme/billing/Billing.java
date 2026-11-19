package com.acme.billing;

import com.acme.money.Money;

/** Charges accounts. */
public class Billing {
    static final int RETRIES = 3;

    private int charged;

    /** Charges {@code amount} once. */
    public Receipt charge(Money amount) {
        charged++;
        log(amount);
        return new Receipt(amount);
    }

    /** Charges {@code amount} with a note. */
    public Receipt charge(Money amount, String note) {
        return charge(amount);
    }

    /** Charges every amount. */
    public Receipt charge(Money... amounts) {
        Money total = new Money(0);
        for (Money amount : amounts) {
            total = total.plus(amount);
        }
        return charge(total, "batch");
    }

    /** The billing every shop starts with. */
    public static Billing standard() {
        return new Billing();
    }

    int retries() {
        return RETRIES;
    }

    private void log(Money amount) {
        charged += 0;
    }
}
