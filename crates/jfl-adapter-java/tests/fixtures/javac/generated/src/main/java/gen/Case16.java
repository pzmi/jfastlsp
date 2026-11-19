package gen;

import gen.other.Item;

public class Case16 {
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

    int Case16;

    void probe0() {
        Object o1 = 1;
        if (o1 instanceof Integer val) {
            int n2 = 1;
            if (n2 > 0) {
                /* { new Base() { int val; */
                // } helper() {
                int v4 = val + 0;
                int h4 = helper();
                int tv4 = this.val;
                int th4 = this.helper();
                Item it4 = null;
                Object ni4 = new Item();
                consume(it4);
            }
        }
    }

    enum En1 {
        A {
            void probe() {
                int v5 = val + 0;
                int h5 = helper();
                int tv5 = this.val;
                int th5 = this.helper();
                Item it5 = null;
                Object ni5 = new Item();
                consume(it5);
            }
        },
        B;

        int val = 5;

        public int helper() {
            return 5;
        }

        void probe() {
        }
    }

}

class Case16Top {
    int y;

    static class Thing {
    }
}

class Case16Mid extends Case16Top {
    private int y;

    private static class Thing {
    }
}
