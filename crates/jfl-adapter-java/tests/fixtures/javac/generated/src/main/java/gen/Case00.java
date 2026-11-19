package gen;

public class Case00 extends Base {
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

    int Case00;

    class In0 implements Constants {
        int val = 3;

        public int helper() {
            return 3;
        }

        void probe() {
            int v1 = val + 0;
            int h1 = helper();
            int th1 = this.helper();
            Item it1 = null;
            Object ni1 = new Item();
            consume(it1);
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

    enum En2 {
        A {
            void probe() {
                int n2 = 1;
                if (n2 > 0) {
                    int t3 = (true ? 1 : 0) + (2);
                    Runnable r4 = () -> {
                        int v5 = val + 0;
                        int h5 = helper();
                        int tv5 = this.val;
                        Item it5 = null;
                        Object ni5 = new Item();
                        consume(it5);
                    };
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

}

class Case00Top {
    int y;

    static class Thing {
    }
}

class Case00Mid extends Case00Top {
    private int y;

    private static class Thing {
    }
}
