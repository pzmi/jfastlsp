package grammar;

import java.util.List;
import java.util.Objects;

/** A point: a record with a compact constructor. */
record Point(int x, int y) {
    Point {
        if (x < 0) {
            throw new IllegalArgumentException("x");
        }
    }

    static Point origin() {
        return new Point(0, 0);
    }

    double length() {
        return Math.sqrt(x * x + y * y);
    }
}

/** A generic record with a varargs component and a nested record. */
record Tagged<T extends Comparable<T>>(T value, String... tags) implements Comparable<Tagged<T>> {
    record Tag(String name) {}

    @Override
    public int compareTo(Tagged<T> other) {
        return value.compareTo(other.value());
    }

    List<Tag> asTags() {
        return java.util.Arrays.stream(tags).map(Tag::new).toList();
    }

    boolean same(Object o) {
        return Objects.equals(this, o);
    }
}
