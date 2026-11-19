package gen;

public class Case17 implements Constants {
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

    int Case17;

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

    void probe1() {
        @SuppressWarnings("val") int a1 = 0;
        Object[] xs2 = { null };
        Base b2 = new Base(xs2[0]) {
            public int helper() {
                return 7;
            }

            public void run() {
                int v3 = val + 0;
                int h3 = helper();
                int tv3 = this.val;
                int th3 = this.helper();
                Item it3 = null;
                Object ni3 = new Item();
                consume(it3);
            }
        };
    }

    interface AdI2 {
        default int m() {
            return 1;
        }
    }

    abstract static class AdB2 {
        abstract int m();
    }

    abstract static class AbsDef2 extends AdB2 implements AdI2 {
        int f() {
            return m();
        }

        int g() {
            return this.m();
        }
    }

    interface AaI2 {
        String m();
    }

    abstract static class AaB2 {
        abstract Object m();
    }

    abstract static class AbsAbs2 extends AaB2 implements AaI2 {
        Object f() {
            return m();
        }
    }

    interface Cov12 {
        Object m();
    }

    interface Cov22 {
        String m();
    }

    abstract static class CovI2 implements Cov12, Cov22 {
        Object f() {
            return m();
        }

        Object g() {
            return this.m();
        }
    }

    interface CovJ2 extends Cov12, Cov22 {
        default Object h() {
            return m();
        }
    }

    interface SaI2 {
        int m();
    }

    interface SaJ2 {
        int m();
    }

    abstract static class SaB2 {
        abstract int m();
    }

    abstract static class SameAbs2 extends SaB2 implements SaI2 {
        int f() {
            return m();
        }
    }

    abstract static class SameAbs22 implements SaI2, SaJ2 {
        int f() {
            return m();
        }
    }

}

class Case17Top {
    int y;

    static class Thing {
    }
}

class Case17Mid extends Case17Top {
    private int y;

    private static class Thing {
    }
}
