package gen;

import gen.other.Item;

public class Case05 implements Constants {
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

    int Case05;

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

    void probe1() {
        int v1 = val + 0;
        int h1 = helper();
        int tv1 = this.val;
        Item it1 = null;
        Object ni1 = new Item();
        consume(it1);
    }

}

class Case05Top {
    int y;

    static class Thing {
    }
}

class Case05Mid extends Case05Top {
    private int y;

    private static class Thing {
    }
}
