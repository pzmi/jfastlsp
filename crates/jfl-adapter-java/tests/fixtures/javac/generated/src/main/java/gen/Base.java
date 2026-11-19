package gen;

public abstract class Base {
    protected int val = 1;

    public Base() {
    }

    public Base(Object o) {
    }

    public Base(Runnable r) {
    }

    public int helper() {
        return 1;
    }

    public static class Item {
    }

    public abstract void run();
}
