package gen;

import gen.other.Item;

public class Case09 {
    int val = 2;

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

    int Case09;

    static class Hd0 extends Item {
        static class Item {
        }

        Item own;
    }

    @SuppressWarnings("Item")
    static class Hb0 implements Comparable<Hb0> {
        public int compareTo(Hb0 other) {
            return 0;
        }
    }

    static class Bd0<T extends Item> {
        static class Item {
        }

        <U extends Item> U pick(T t, U u) {
            return u;
        }
    }

    enum En1 {
        A {
            void probe() {
                {
                    int v2 = val + 0;
                    int h2 = helper();
                    int tv2 = this.val;
                    int th2 = this.helper();
                    Item it2 = null;
                    Object ni2 = new Item();
                    consume(it2);
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

    static class St2 extends Base {
        int val = 4;

        public int helper() {
            return 4;
        }

        public void run() {
        }

        void probe() {
            Object[] xs3 = { null };
            Base b3 = new Base(() -> { }) {
                public int helper() {
                    return 7;
                }

                public void run() {
                    for (int i4 = 0; i4 < 1; i4++) {
                        int v5 = val + 0;
                        int h5 = helper();
                        int tv5 = this.val;
                        Item it5 = null;
                        Object ni5 = new Item();
                        consume(it5);
                    }
                }
            };
        }
    }

    interface AdI3 {
        default int m() {
            return 1;
        }
    }

    abstract static class AdB3 {
        abstract int m();
    }

    abstract static class AbsDef3 extends AdB3 implements AdI3 {
        int f() {
            return m();
        }

        int g() {
            return this.m();
        }
    }

    interface AaI3 {
        String m();
    }

    abstract static class AaB3 {
        abstract Object m();
    }

    abstract static class AbsAbs3 extends AaB3 implements AaI3 {
        Object f() {
            return m();
        }
    }

    interface Cov13 {
        Object m();
    }

    interface Cov23 {
        String m();
    }

    abstract static class CovI3 implements Cov13, Cov23 {
        Object f() {
            return m();
        }

        Object g() {
            return this.m();
        }
    }

    interface CovJ3 extends Cov13, Cov23 {
        default Object h() {
            return m();
        }
    }

    interface SaI3 {
        int m();
    }

    interface SaJ3 {
        int m();
    }

    abstract static class SaB3 {
        abstract int m();
    }

    abstract static class SameAbs3 extends SaB3 implements SaI3 {
        int f() {
            return m();
        }
    }

    abstract static class SameAbs23 implements SaI3, SaJ3 {
        int f() {
            return m();
        }
    }

}

class Case09Top {
    int y;

    static class Thing {
    }
}

class Case09Mid extends Case09Top {
    private int y;

    private static class Thing {
    }
}
