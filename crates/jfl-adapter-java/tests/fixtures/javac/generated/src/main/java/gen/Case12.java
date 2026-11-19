package gen;

import gen.other.Item;

public class Case12 implements Constants {
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

    int Case12;

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

    class PIn1 extends gen.other.PBase1 {
        int f() {
            return z;
        }

        Object g() {
            return new Gadget();
        }
    }

}

class Case12Top {
    int y;

    static class Thing {
    }
}

class Case12Mid extends Case12Top {
    private int y;

    private static class Thing {
    }
}
