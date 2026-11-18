// This file exists to fail: it holds syntax errors on purpose (fixtures/java/broken/, ver 4).
package broken;

class SyntaxErrors {
    int missingSemicolon = 1
    int next;

    void stray() {
        int x = ) 2;
        call(x;
    }

    void fine() {
        stray();
    }
