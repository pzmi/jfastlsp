package com.acme.shop;

import com.acme.billing.Billing;

/** A subclass in another package: Billing's public members are inherited, its package-private ones are not. */
class Discount extends Billing {
    Billing again() {
        return standard();
    }
}
