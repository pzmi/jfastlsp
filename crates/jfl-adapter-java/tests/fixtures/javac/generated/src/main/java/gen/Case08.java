package gen;

import gen.other.Item;

public class Case08 extends Base {
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

    int Case08;

    class PIn0 extends gen.other.PBase1 {
        int f() {
            return z;
        }

        Object g() {
            return new Gadget();
        }
    }

    interface HK1 {
        int k = 1;
    }

    static class HKBase1 implements HK1 {
        private static final int k = 2;
    }

    class HKIn1 extends HKBase1 {
        int f() {
            return k;
        }
    }

    class TIn1 extends Case08Mid {
        int f() {
            return y;
        }

        Thing g() {
            return null;
        }
    }

    class PIn2 extends gen.other.PBase1 {
        int f() {
            return z;
        }

        Object g() {
            return new Gadget();
        }
    }

}

class Case08Top {
    int y;

    static class Thing {
    }
}

class Case08Mid extends Case08Top {
    private int y;

    private static class Thing {
    }
}
