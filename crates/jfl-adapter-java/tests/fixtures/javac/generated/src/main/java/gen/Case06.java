package gen;

public class Case06 implements Constants {
    int val = 2;

    public int helper() {
        return 2;
    }

    static void consume(Object o) {
    }

    static class Entry<K, V> {
    }

    int modCount;

    public int size() {
        return 0;
    }

    int z;

    static class Gadget {
    }

    int k;

    int y;

    static class Thing {
    }

    int Case06;

    class Lib0 extends java.util.HashMap<String, String> {
        void probe() {
            int s0 = size() + modCount;
            Entry<String, String> e0 = null;
            consume(e0);
        }
    }

    void probe1() {
        consume(new Base() {
            public void run() {
                @SuppressWarnings("val") int a2 = 0;
                Object[] xs3 = { null };
                Base b3 = new Base(() -> { }) {
                    public int helper() {
                        return 7;
                    }

                    public void run() {
                        int v4 = val + 0;
                        int h4 = helper();
                        int th4 = this.helper();
                        Item it4 = null;
                        Object ni4 = new Item();
                        consume(it4);
                    }
                };
            }
        });
    }

}

class Case06Top {
    int y;

    static class Thing {
    }
}

class Case06Mid extends Case06Top {
    private int y;

    private static class Thing {
    }
}
