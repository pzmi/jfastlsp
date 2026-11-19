package gen;

import gen.other.Item;

public class Case26 {
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

    int Case26;

    enum En0 {
        A {
            void probe() {
                Runnable r1 = () -> {
                    Object[] arr2 = {
                        new Base() {
                            public void run() {
                                int v3 = val + 0;
                                int h3 = helper();
                                int tv3 = this.val;
                                int th3 = this.helper();
                                Item it3 = null;
                                Object ni3 = new Item();
                                consume(it3);
                            }
                        }
                    };
                };
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
        Object o4 = 1;
        if (o4 instanceof Integer val) {
            Object[] arr5 = {
                new Base() {
                    public void run() {
                        String s6 = "} { new Base() { val helper()";
                        int v7 = val + 0;
                        int h7 = helper();
                        int tv7 = this.val;
                        Item it7 = null;
                        Object ni7 = new Item();
                        consume(it7);
                    }
                }
            };
        }
    }

}

class Case26Top {
    int y;

    static class Thing {
    }
}

class Case26Mid extends Case26Top {
    private int y;

    private static class Thing {
    }
}
