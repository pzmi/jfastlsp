package com.acme.probe;

import java.sql.Date;

/** `at(Date)` here takes java.sql.Date: it overloads SuperDates#at, it does not override it. */
class SubDates extends SuperDates {
    void at(Date d) {}

    void call(java.util.Date u) {
        at(u);
    }
}
