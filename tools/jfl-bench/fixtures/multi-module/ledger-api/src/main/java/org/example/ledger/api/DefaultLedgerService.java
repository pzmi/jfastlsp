package org.example.ledger.api;

import org.example.ledger.core.Account;
import org.example.ledger.core.Currency;
import org.example.ledger.core.Entry;
import org.example.ledger.core.Ledger;
import org.example.ledger.core.Money;

/** A ledger service over one in-memory ledger. */
public final class DefaultLedgerService implements LedgerService {
    private final Ledger ledger = new Ledger();

    @Override
    public Account open(String id, Currency currency) {
        return ledger.open(id, currency);
    }

    @Override
    public void transfer(TransferRequest request) {
        Money amount = request.amount();
        Entry debit = new Entry(request.from(), amount.negate(), request.memo());
        Entry credit = new Entry(request.to(), amount, request.memo());
        ledger.post(debit, credit);
    }

    @Override
    public Money balance(String accountId) {
        return ledger.account(accountId).balance();
    }
}
