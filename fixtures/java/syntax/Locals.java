package syntax;

import java.io.StringReader;
import java.util.List;

/** Uses bound to locals and parameters are not reference sites; unclear bindings stay. */
class Locals {
    int value;
    int count;

    int params(int value, List<String> list) {
        int sum = value + this.value;
        for (int i = 0; i < list.size(); i++) {
            sum += i;
        }
        for (String item : list) {
            sum += item.length();
        }
        list.forEach(entry -> System.out.println(entry + count));
        return sum;
    }

    int beforeDeclaration() {
        int before = count;
        int count = 2;
        return before + count;
    }

    Object shadowedByAField(int count) {
        return new Object() {
            int count = 3;

            @Override
            public String toString() {
                return "" + count;
            }
        };
    }

    String resources() throws Exception {
        try (StringReader reader = new StringReader("x")) {
            return String.valueOf(reader.read());
        } catch (IllegalStateException error) {
            return error.getMessage();
        }
    }

    int patterns(Object o) {
        if (!(o instanceof String text)) {
            return 0;
        }
        return text.length();
    }

    int lambdas(List<Integer> numbers) {
        return numbers.stream().map((Integer n) -> n + value).reduce(0, (a, b) -> a + b);
    }

    int annotationKeys(int value, @Key(value = 1) int y) {
        int other = value;
        @Key(value = 2) int local = other;
        return local + y;
    }

    int enumLabels(Color c) {
        int RED = 2;
        switch (c) {
            case RED:
                return RED;
            case GREEN, BLUE:
                return 0;
            default:
                return 1;
        }
    }

    String switchPatterns(Object o) {
        return switch (o) {
            case Point(int x, int y) when x > y -> "p" + x + y;
            case String s -> s;
            default -> "";
        };
    }

    Runnable nested(int base) {
        return () -> {
            java.util.function.IntUnaryOperator add = n -> n + base;
            loop:
            for (int i = 0; i < 2; i++) {
                if (add.applyAsInt(i) > 0) {
                    break loop;
                }
            }
        };
    }

    Object classesReadOuterLocals(int outer) {
        int captured = outer + 1;
        class Local {
            int read() {
                return captured;
            }
        }
        return new Object() {
            @Override
            public String toString() {
                return "" + captured + new Local().read();
            }
        };
    }

    String varResourceAndMultiCatch() {
        try (var reader = new StringReader("x")) {
            return String.valueOf(reader.read());
        } catch (IllegalStateException | java.io.IOException failure) {
            return failure.getMessage();
        }
    }
}

@interface Key {
    int value();
}

enum Color {
    RED,
    GREEN,
    BLUE
}

record Point(int x, int y) {}
