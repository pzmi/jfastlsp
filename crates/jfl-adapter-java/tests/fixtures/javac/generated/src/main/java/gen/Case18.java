package gen;

import gen.other.Item;

public class Case18 extends Base {
    int val = 2;

    public int helper() {
        return 2;
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

    int Case18;

    static class HTop0 {
        int val;

        static class Item {
        }
    }

    static class HMid0 extends HTop0 {
        private int val;

        private static class Item {
        }
    }

    class HIn0 extends HMid0 {
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

    static class St1 extends Base {
        int val = 4;

        public int helper() {
            return 4;
        }

        public void run() {
        }

        void probe() {
            {
                int val = 9;
                int v3 = val + 0;
                int h3 = helper();
                Item it3 = null;
                Object ni3 = new Item();
                consume(it3);
            }
        }
    }

    static class Hd2 extends Item {
        static class Item {
        }

        Item own;
    }

    @SuppressWarnings("Item")
    static class Hb2 implements Comparable<Hb2> {
        public int compareTo(Hb2 other) {
            return 0;
        }
    }

    static class Bd2<T extends Item> {
        static class Item {
        }

        <U extends Item> U pick(T t, U u) {
            return u;
        }
    }

}

class Case18Top {
    int y;

    static class Thing {
    }
}

class Case18Mid extends Case18Top {
    private int y;

    private static class Thing {
    }
}
