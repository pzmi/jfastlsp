package gen;

public class Case02 implements Constants {
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

    int Case02;

    class PIn0 extends gen.other.PBase1 {
        int f() {
            return z;
        }

        Object g() {
            return new Gadget();
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

class Case02Top {
    int y;

    static class Thing {
    }
}

class Case02Mid extends Case02Top {
    private int y;

    private static class Thing {
    }
}
