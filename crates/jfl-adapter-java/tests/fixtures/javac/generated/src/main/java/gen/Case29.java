package gen;

public class Case29 implements Constants {
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

    int Case29;

    @SuppressWarnings("Gen0")
    class Gen0<Item> {
        void probe() {
            /* { new Base() { int val; */
            // } helper() {
            int v2 = val + 0;
            int h2 = helper();
            Item it2 = null;
            consume(it2);
        }
    }

    static class HTop1 {
        int val;

        static class Item {
        }
    }

    static class HMid1 extends HTop1 {
        private int val;

        private static class Item {
        }
    }

    class HIn1 extends HMid1 {
        int f() {
            return val;
        }

        Item g() {
            return null;
        }

        Object n() {
            return new Item();
        }
    }

}

class Case29Top {
    int y;

    static class Thing {
    }
}

class Case29Mid extends Case29Top {
    private int y;

    private static class Thing {
    }
}
