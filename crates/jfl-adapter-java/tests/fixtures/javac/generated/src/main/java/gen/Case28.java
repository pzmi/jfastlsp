package gen;

import gen.other.Item;

public class Case28 implements Constants {
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

    int Case28;

    static class St0 extends Base {
        int val = 4;

        public int helper() {
            return 4;
        }

        public void run() {
        }

        void probe() {
            Object[] arr1 = {
                new Base() {
                    public void run() {
                        int v2 = val + 0;
                        int h2 = helper();
                        int tv2 = this.val;
                        Item it2 = null;
                        Object ni2 = new Item();
                        consume(it2);
                    }
                }
            };
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

    void probe2() {
        Runnable r3 = () -> {
            int v4 = val + 0;
            int h4 = helper();
            int th4 = this.helper();
            Item it4 = null;
            Object ni4 = new Item();
            consume(it4);
        };
    }

}

class Case28Top {
    int y;

    static class Thing {
    }
}

class Case28Mid extends Case28Top {
    private int y;

    private static class Thing {
    }
}
