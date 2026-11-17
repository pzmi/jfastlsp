package org.example.ledger.app;

import java.io.PrintStream;
import org.example.ledger.api.LedgerService;
import org.example.ledger.core.Money;

/** Prints balances. */
final class ReportPrinter {
    private final PrintStream out;

    ReportPrinter(PrintStream out) {
        this.out = out;
    }

    void print(LedgerService service, String... accountIds) {
        for (String id : accountIds) {
            Money balance = service.balance(id);
            out.println(id + " " + balance.minor() + " " + balance.currency());
        }
    }
}
