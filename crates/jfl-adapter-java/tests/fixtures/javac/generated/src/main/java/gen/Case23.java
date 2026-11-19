package gen;

import gen.other.Item;

public class Case23 implements Constants {
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

    int Case23;

    void probe0() {
        class Loc1 extends Base {
            int val = 8;

            public void run() {
                int v2 = val + 0;
                int h2 = helper();
                int tv2 = this.val;
                int th2 = this.helper();
                Item it2 = null;
                Object ni2 = new Item();
                consume(it2);
            }
        }
        new Loc1().run();
    }

    class PIn1 extends gen.other.PBase1 {
        int f() {
            return z;
        }

        Object g() {
            return new Gadget();
        }
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

class Case23Top {
    int y;

    static class Thing {
    }
}

class Case23Mid extends Case23Top {
    private int y;

    private static class Thing {
    }
}
