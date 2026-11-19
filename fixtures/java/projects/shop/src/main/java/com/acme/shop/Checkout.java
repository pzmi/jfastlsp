package com.acme.shop;

import static com.acme.billing.Billing.standard;

import com.acme.billing.Receipt;
import com.acme.money.Money;

final class Checkout {
    private Checkout() {}

    static Receipt pay(Money amount) {
        return standard().charge(amount);
    }
}
