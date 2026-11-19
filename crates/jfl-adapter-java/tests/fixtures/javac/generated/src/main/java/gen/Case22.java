package gen;

public class Case22 extends Base {
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

    int Case22;

    class PIn0 extends gen.other.PBase1 {
        int f() {
            return z;
        }

        Object g() {
            return new Gadget();
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

class Case22Top {
    int y;

    static class Thing {
    }
}

class Case22Mid extends Case22Top {
    private int y;

    private static class Thing {
    }
}
