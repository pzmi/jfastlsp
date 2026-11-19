package gen;

public class Case25 implements Constants {
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

    int Case25;

    class Q0 {
        int val;

        int f() {
            return Case25.this.val + Case25.this.helper();
        }
    }

    record Rec0(int val) {
        int g() {
            return val() + val;
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

    static class HTop2 {
        int val;

        static class Item {
        }
    }

    static class HMid2 extends HTop2 {
        private int val;

        private static class Item {
        }
    }

    class HIn2 extends HMid2 {
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

class Case25Top {
    int y;

    static class Thing {
    }
}

class Case25Mid extends Case25Top {
    private int y;

    private static class Thing {
    }
}
