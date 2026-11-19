package gen;

import gen.other.Item;

public class Case15 implements Constants {
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

    int Case15;

    enum En0 {
        A {
            void probe() {
                consume(new Base() {
                    int val = 6;

                    public void run() {
                        try {
                            int v3 = val + 0;
                            int h3 = helper();
                            int th3 = this.helper();
                            Item it3 = null;
                            Object ni3 = new Item();
                            consume(it3);
                        } catch (RuntimeException e2) {
                        }
                    }
                });
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

    enum En1 {
        A {
            void probe() {
                String s4 = "} { new Base() { val helper()";
                Object[] xs5 = { null };
                Base b5 = new Base(new Object()) {
                    public int helper() {
                        return 7;
                    }

                    public void run() {
                        int v6 = val + 0;
                        int h6 = helper();
                        int tv6 = this.val;
                        int th6 = this.helper();
                        Item it6 = null;
                        Object ni6 = new Item();
                        consume(it6);
                    }
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

    enum En2 {
        A {
            void probe() {
                for (int val : new int[] {1}) {
                    int t8 = (true ? 1 : 0) + (2);
                    @SuppressWarnings("val") int a9 = 0;
                    int v10 = val + 0;
                    int h10 = helper();
                    int tv10 = this.val;
                    int th10 = this.helper();
                    Item it10 = null;
                    Object ni10 = new Item();
                    consume(it10);
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

class Case15Top {
    int y;

    static class Thing {
    }
}

class Case15Mid extends Case15Top {
    private int y;

    private static class Thing {
    }
}
