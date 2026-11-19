package gen;

import gen.other.Item;

public class Case20 {
    int val = 2;

    public int helper() {
        return 2;
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

    int Case20;

    class Q0 {
        int val;

        int f() {
            return Case20.this.val + Case20.this.helper();
        }
    }

    record Rec0(int val) {
        int g() {
            return val() + val;
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

class Case20Top {
    int y;

    static class Thing {
    }
}

class Case20Mid extends Case20Top {
    private int y;

    private static class Thing {
    }
}
