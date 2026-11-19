package com.acme.probe3;

/** The slice-2 review's H2: `QualThis.this` names the class, never the field named like it. */
class QualThis {
    int QualThis;
    int count;

    void m() {
    }

    class Inner {
        int count;

        int f() {
            return QualThis.this.count;
        }

        void g() {
            QualThis.this.m();
        }
    }
}
