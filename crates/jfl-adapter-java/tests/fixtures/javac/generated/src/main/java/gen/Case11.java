package gen;

public class Case11 implements Constants {
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

    int Case11;

    void probe0() {
        int v1 = val + 0;
        int h1 = helper();
        int tv1 = this.val;
        int th1 = this.helper();
        Item it1 = null;
        Object ni1 = new Item();
        consume(it1);
    }

    void probe1() {
        int val = 9;
        String s3 = "} { new Base() { val helper()";
        int v4 = val + 0;
        int h4 = helper();
        int th4 = this.helper();
        Item it4 = null;
        Object ni4 = new Item();
        consume(it4);
    }

}

class Case11Top {
    int y;

    static class Thing {
    }
}

class Case11Mid extends Case11Top {
    private int y;

    private static class Thing {
    }
}
