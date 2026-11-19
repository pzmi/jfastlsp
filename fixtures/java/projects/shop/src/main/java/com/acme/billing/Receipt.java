package com.acme.billing;

import com.acme.money.Money;

/** What a charge returns. */
public record Receipt(Money amount) {}
