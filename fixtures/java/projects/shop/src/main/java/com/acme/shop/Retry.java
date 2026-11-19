package com.acme.shop;

import com.acme.billing.Billing;

/** Billing's package-private retries() is not inherited outside its package: Again calls Retry's. */
class Retry {
    int retries() {
        return 0;
    }

    class Again extends Billing {
        int count() {
            return retries();
        }
    }
}
