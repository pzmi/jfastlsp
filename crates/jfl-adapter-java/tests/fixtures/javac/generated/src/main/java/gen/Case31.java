package gen;

import gen.other.Item;

public class Case31 extends Base {
    public int helper() {
        return 2;
    }

    public int helper(int x) {
        return x;
    }

    static class Item {
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

    int Case31;

    class In0 implements Constants {
        public int helper() {
            return 3;
        }

        static class Item {
        }

        void probe() {
            Runnable r1 = () -> {
                class Loc2 extends Base {
                    int val = 8;

                    public void run() {
                        /* { new Base() { int val; */
                        // } helper() {
                        int v4 = val + 0;
                        int h4 = helper();
                        int tv4 = this.val;
                        int th4 = this.helper();
                        Item it4 = null;
                        Object ni4 = new Item();
                        consume(it4);
                    }
                }
                new Loc2().run();
            };
        }
    }

    void probe1() {
        int v5 = val + 0;
        int h5 = helper();
        int tv5 = this.val;
        int th5 = this.helper();
        Item it5 = null;
        Object ni5 = new Item();
        consume(it5);
    }

    static class St2 extends Base {
        int val = 4;

        public int helper() {
            return 4;
        }

        public void run() {
        }

        void probe() {
            @SuppressWarnings("val") int a6 = 0;
            consume(new Base() {
                int val = 6;

                public void run() {
                    Runnable r8 = () -> {
                        int v9 = val + 0;
                        int h9 = helper();
                        int tv9 = this.val;
                        Item it9 = null;
                        Object ni9 = new Item();
                        consume(it9);
                    };
                }
            });
        }
    }

}

class Case31Top {
    int y;

    static class Thing {
    }
}

class Case31Mid extends Case31Top {
    private int y;

    private static class Thing {
    }
}
