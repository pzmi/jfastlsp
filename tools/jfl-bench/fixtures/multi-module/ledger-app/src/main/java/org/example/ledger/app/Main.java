package org.example.ledger.app;

import org.example.ledger.api.DefaultLedgerService;
import org.example.ledger.api.LedgerService;
import org.example.ledger.api.TransferRequest;
import org.example.ledger.core.Currency;
import org.example.ledger.core.Money;

/** Opens two accounts, moves money between them and prints the balances. */
public final class Main {
    private Main() {}

    public static void main(String[] args) {
        LedgerService service = new DefaultLedgerService();
        service.open("cash", Currency.EUR);
        service.open("bank", Currency.EUR);
        service.transfer(new TransferRequest("cash", "bank", new Money(1250, Currency.EUR), "deposit"));
        new ReportPrinter(System.out).print(service, "cash", "bank");
    }
}
