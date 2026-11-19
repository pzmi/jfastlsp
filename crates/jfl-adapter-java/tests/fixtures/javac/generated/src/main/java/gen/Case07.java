package gen;

public class Case07 {
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

    int Case07;

    static class HTop0 {
        int val;

        static class Item {
        }
    }

    static class HMid0 extends HTop0 {
        private int val;

        private static class Item {
        }
    }

    class HIn0 extends HMid0 {
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

    class In1 {
        int val = 3;

        void probe() {
            int n1 = 1;
            switch (n1) {
                case 1 -> {
                    int v2 = val + 0;
                    int h2 = helper();
                    Item it2 = null;
                    Object ni2 = new Item();
                    consume(it2);
                }
                default -> {
                }
            }
        }
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

class Case07Top {
    int y;

    static class Thing {
    }
}

class Case07Mid extends Case07Top {
    private int y;

    private static class Thing {
    }
}
