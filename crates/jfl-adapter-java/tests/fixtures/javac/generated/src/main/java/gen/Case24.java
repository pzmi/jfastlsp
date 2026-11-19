package gen;

public class Case24 extends Base {
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

    int Case24;

    enum En0 {
        A {
            void probe() {
                int val = 9;
                int v2 = val + 0;
                int h2 = helper();
                int tv2 = this.val;
                int th2 = this.helper();
                Item it2 = null;
                Object ni2 = new Item();
                consume(it2);
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

    void probe1() {
        int n3 = 1;
        switch (n3) {
            case 1 -> {
                int v4 = val + 0;
                int h4 = helper();
                int tv4 = this.val;
                int th4 = this.helper();
                Item it4 = null;
                Object ni4 = new Item();
                consume(it4);
            }
            default -> {
            }
        }
    }

    static class St2 {
        int val = 4;

        public int helper() {
            return 4;
        }

        void probe() {
            int v5 = val + 0;
            int h5 = helper();
            int tv5 = this.val;
            int th5 = this.helper();
            Item it5 = null;
            Object ni5 = new Item();
            consume(it5);
        }
    }

    class Q3 {
        int val;

        int f() {
            return Case24.this.val + Case24.this.helper();
        }
    }

    record Rec3(int val) {
        int g() {
            return val() + val;
        }
    }

}

class Case24Top {
    int y;

    static class Thing {
    }
}

class Case24Mid extends Case24Top {
    private int y;

    private static class Thing {
    }
}
