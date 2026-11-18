package grammar;

/** Java 25 (JEP 513): statements before an explicit constructor invocation. */
class FlexibleConstructorBodies extends Base {
    private final int size;

    FlexibleConstructorBodies(int size) {
        if (size < 0) {
            throw new IllegalArgumentException("size");
        }
        int checked = Math.max(size, 1);
        super(checked);
        this.size = checked;
    }

    FlexibleConstructorBodies() {
        this(1);
    }
}

class Base {
    Base(int capacity) {}
}
