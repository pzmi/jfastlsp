package gen;

import gen.other.Item;

public class Case01 extends Base {
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

    int Case01;

    enum En0 {
        A {
            void probe() {
                String s1 = "} { new Base() { val helper()";
                int n2 = 1;
                if (n2 > 0) {
                    int v3 = val + 0;
                    int h3 = helper();
                    int tv3 = this.val;
                    int th3 = this.helper();
                    Item it3 = null;
                    Object ni3 = new Item();
                    consume(it3);
                }
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

}

class Case01Top {
    int y;

    static class Thing {
    }
}

class Case01Mid extends Case01Top {
    private int y;

    private static class Thing {
    }
}
