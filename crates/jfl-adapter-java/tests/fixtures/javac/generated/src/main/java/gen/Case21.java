package gen;

import gen.other.Item;

public class Case21 extends Base {
    int val = 2;

    public int helper() {
        return 2;
    }

    public int helper(int x) {
        return x;
    }

    public void run() {
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

    int Case21;

    class Lib0 extends java.util.HashMap<String, String> {
        void probe() {
            int s0 = size() + modCount;
            Entry<String, String> e0 = null;
            consume(e0);
        }
    }

    class In1 {
        public int helper() {
            return 3;
        }

        static class Item {
        }

        void probe() {
            Object o1 = 1;
            if (o1 instanceof Integer val) {
                @SuppressWarnings("val") int a2 = 0;
                int v3 = val + 0;
                int h3 = helper();
                int th3 = this.helper();
                Item it3 = null;
                Object ni3 = new Item();
                consume(it3);
            }
        }
    }

}

class Case21Top {
    int y;

    static class Thing {
    }
}

class Case21Mid extends Case21Top {
    private int y;

    private static class Thing {
    }
}
