package gen;

public class Case10 extends Base {
    public int helper() {
        return 2;
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

    int Case10;

    class PIn0 extends gen.other.PBase1 {
        int f() {
            return z;
        }

        Object g() {
            return new Gadget();
        }
    }

    void probe1() {
        int v1 = val + 0;
        int h1 = helper();
        int tv1 = this.val;
        int th1 = this.helper();
        Item it1 = null;
        Object ni1 = new Item();
        consume(it1);
    }

    static class HTop2 {
        int val;

        static class Item {
        }
    }

    static class HMid2 extends HTop2 {
        private int val;

        private static class Item {
        }
    }

    class HIn2 extends HMid2 {
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

}

class Case10Top {
    int y;

    static class Thing {
    }
}

class Case10Mid extends Case10Top {
    private int y;

    private static class Thing {
    }
}
