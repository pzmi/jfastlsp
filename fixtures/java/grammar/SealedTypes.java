package grammar;

/** A sealed hierarchy: an interface that permits three implementations. */
sealed interface Shape permits Circle, Square, Polygon {
    double area();
}

final class Circle implements Shape {
    private final double radius;

    Circle(double radius) {
        this.radius = radius;
    }

    @Override
    public double area() {
        return Math.PI * radius * radius;
    }
}

record Square(double side) implements Shape {
    @Override
    public double area() {
        return side * side;
    }
}

sealed abstract class Polygon implements Shape permits Triangle, Polygon.Other {
    static non-sealed class Other extends Polygon {
        @Override
        public double area() {
            return 0;
        }
    }
}

final class Triangle extends Polygon {
    @Override
    public double area() {
        return 1;
    }
}
