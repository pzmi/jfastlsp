package com.acme.shop;

import com.acme.billing.Billing;
import com.acme.billing.Receipt;
import com.acme.money.*;
import java.util.ArrayList;
import java.util.List;

/** The shop front. */
public class Shop {
    private final Billing billing = Billing.standard();

    private final List<Receipt> receipts = new ArrayList<>();

    /** Charges the cart's total and keeps the receipt. */
    public Receipt checkout(Cart cart) {
        Receipt receipt = billing.charge(cart.total());
        receipts.add(receipt);
        this.audit(receipt);
        return receipt;
    }

    void audit(Receipt receipt) {
        Money amount = receipt.amount();
    }

    int size() {
        return receipts.size();
    }

    Runnable task() {
        return new Runnable() {
            @Override
            public void run() {
                size();
            }
        };
    }

    /** Counts checkouts. */
    static class Counter {
        int count;

        void bump() {
            count++;
        }
    }
}
