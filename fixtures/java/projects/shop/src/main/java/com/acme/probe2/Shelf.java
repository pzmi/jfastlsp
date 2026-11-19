package com.acme.probe2;

/** Type parameters named like a member type, with and without an annotation before them. */
class Shelf {
    static class Row {}

    @SuppressWarnings("Tray")
    static class Tray<Row> {
        Row first;
    }

    static class Rack<Row> {
        Row first;
    }
}
