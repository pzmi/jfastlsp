package com.acme.shop;

import com.acme.billing.Billing;
import com.acme.billing.Receipt;
import com.acme.money.Money;

/** A field named like its type obscures it: Java reads {@code Billing.charge} as the field's. */
final class Obscure {
    private final Billing Billing = new Billing();

    Receipt pay(Money amount) {
        return Billing.charge(amount);
    }
}
