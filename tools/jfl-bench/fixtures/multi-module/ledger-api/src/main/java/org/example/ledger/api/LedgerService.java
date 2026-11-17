package org.example.ledger.api;

import org.example.ledger.core.Account;
import org.example.ledger.core.Currency;
import org.example.ledger.core.Money;

/** What clients may do with a ledger. */
public interface LedgerService {
    Account open(String id, Currency currency);

    void transfer(TransferRequest request);

    Money balance(String accountId);
}
