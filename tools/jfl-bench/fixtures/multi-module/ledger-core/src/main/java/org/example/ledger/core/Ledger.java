package org.example.ledger.core;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Accounts and the entries posted to them; every transfer balances to zero. */
public final class Ledger {
    private final Map<String, Account> accounts = new LinkedHashMap<>();
    private final List<Entry> journal = new ArrayList<>();

    public Account open(String id, Currency currency) {
        return accounts.computeIfAbsent(id, key -> new Account(key, currency));
    }

    public Account account(String id) {
        Account account = accounts.get(id);
        if (account == null) {
            throw new IllegalArgumentException("no account " + id);
        }
        return account;
    }

    public void post(Entry debit, Entry credit) {
        Money sum = debit.amount().plus(credit.amount());
        if (sum.minor() != 0) {
            throw new IllegalStateException("unbalanced transfer");
        }
        account(debit.accountId()).apply(debit);
        account(credit.accountId()).apply(credit);
        journal.add(debit);
        journal.add(credit);
    }

    public List<Entry> journal() {
        return List.copyOf(journal);
    }
}
