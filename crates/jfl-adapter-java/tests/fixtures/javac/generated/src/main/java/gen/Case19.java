package gen;

import gen.other.Item;

public class Case19 {
    int val = 2;

    public int helper() {
        return 2;
    }

    public int helper(int x) {
        return x;
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

    int Case19;

    void probe0() {
        class Loc1 extends Base {
            public void run() {
                /* { new Base() { int val; */
                // } helper() {
                int v3 = val + 0;
                int h3 = helper();
                int tv3 = this.val;
                int th3 = this.helper();
                Item it3 = null;
                Object ni3 = new Item();
                consume(it3);
            }
        }
        new Loc1().run();
    }

    static class Hd1 extends Item {
        static class Item {
        }

        Item own;
    }

    @SuppressWarnings("Item")
    static class Hb1 implements Comparable<Hb1> {
        public int compareTo(Hb1 other) {
            return 0;
        }
    }

    static class Bd1<T extends Item> {
        static class Item {
        }

        <U extends Item> U pick(T t, U u) {
            return u;
        }
    }

    class In2 extends Base {
        int val = 3;

        public int helper() {
            return 3;
        }

        static class Item {
        }

        public void run() {
        }

        void probe() {
            Object[] xs4 = { null };
            Base b4 = new Base(new Object()) {
                public int helper() {
                    return 7;
                }

                public void run() {
                    @SuppressWarnings("val") int a5 = 0;
                    try {
                        int v7 = val + 0;
                        int h7 = helper();
                        int tv7 = this.val;
                        Item it7 = null;
                        Object ni7 = new Item();
                        consume(it7);
                    } catch (RuntimeException e6) {
                    }
                }
            };
        }
    }

    interface HK3 {
        int k = 1;
    }

    static class HKBase3 implements HK3 {
        private static final int k = 2;
    }

    class HKIn3 extends HKBase3 {
        int f() {
            return k;
        }
    }

    class TIn3 extends Case19Mid {
        int f() {
            return y;
        }

        Thing g() {
            return null;
        }
    }

}

class Case19Top {
    int y;

    static class Thing {
    }
}

class Case19Mid extends Case19Top {
    private int y;

    private static class Thing {
    }
}
