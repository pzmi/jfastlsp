package org.example.ledger.api;

import org.example.ledger.core.Money;

/** A request to move an amount from one account to another. */
public record TransferRequest(String from, String to, Money amount, String memo) {}
