package syntax;

/** Names spelled with Unicode escapes and identifier-ignorable characters (JLS 3.3, 3.8). */
class UnicodeEscapes {
    int \u0063ount;
    int na​me;
    String quote = "\u0041 in a string";
    char nul = '\u0000'; // \u0000 in a comment stays as written

    int read() {
        return count + name + c\u006Funt;
    }

    void \uuu0072un() {
        String escaped = "\\u0041";
        System.out.println(escaped + quote + été());
    }

    int été() {
        return 1;
    }
}
