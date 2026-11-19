package com.acme.billing;

import com.acme.money.Money;

/** Billing with a discount. */
public class PremiumBilling extends Billing {
    /** Charges {@code amount} less {@code percent}. */
    public Receipt charge(Money amount, int percent) {
        return charge(amount);
    }

    int twice() {
        return retries() * 2;
    }
}
