package gen;

public class Case30 {
    int val = 2;

    public int helper() {
        return 2;
    }

    public int helper(int x) {
        return x;
    }

    static class Item {
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

    int Case30;

    class PIn0 extends gen.other.PBase1 {
        int f() {
            return z;
        }

        Object g() {
            return new Gadget();
        }
    }

    void probe1() {
        try {
            int v2 = val + 0;
            int h2 = helper();
            Item it2 = null;
            Object ni2 = new Item();
            consume(it2);
        } catch (RuntimeException e1) {
        }
    }

    class Q2 {
        int val;

        int f() {
            return Case30.this.val + Case30.this.helper();
        }
    }

    record Rec2(int val) {
        int g() {
            return val() + val;
        }
    }

}

class Case30Top {
    int y;

    static class Thing {
    }
}

class Case30Mid extends Case30Top {
    private int y;

    private static class Thing {
    }
}
