package org.example.ledger.core;

/** One posting against one account. */
public record Entry(String accountId, Money amount, String memo) {}
