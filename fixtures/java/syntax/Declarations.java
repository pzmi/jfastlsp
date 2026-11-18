package syntax;

import java.io.Serializable;
import java.util.List;
import java.util.function.Supplier;

/** Every declaration kind, with modifiers, generics and erasure. */
public abstract class Declarations<K extends Comparable<K> & Serializable, V> implements Serializable {
    public static final int A = 1, B[] = {2}, C = 3;
    protected transient volatile List<? extends V> values;
    private int matrix[][];

    static {
        class InInitializer {
            int hidden;
        }
        new InInitializer();
    }

    protected Declarations(K key, V... values) {}

    public abstract <T extends Number> T convert(K key, List<T> into, T[] array) throws Exception;

    synchronized <V2 extends V> void put(V2 value, int... counts) {}

    static native long address();

    strictfp double exact(double d) {
        return d;
    }

    int overloaded(int i) {
        return i;
    }

    int overloaded(Integer i) {
        return i;
    }

    void locals() {
        class Local {
            void inLocal() {}
        }
        Supplier<Object> anonymous = new Supplier<>() {
            @Override
            public Object get() {
                return new Local();
            }
        };
        anonymous.get();
    }

    /** A member interface with a constant and a default method. */
    interface Inner<E> {
        String CONSTANT = "c";

        E element();

        default boolean empty() {
            return element() == null;
        }
    }

    enum Planet implements Inner<String> {
        MERCURY(1) {
            @Override
            public String element() {
                return "Hg";
            }
        },
        VENUS(2);

        private final int order;

        Planet(int order) {
            this.order = order;
        }

        @Override
        public String element() {
            return name();
        }
    }

    @interface Config {
        String name() default "x";

        int[] sizes();
    }

    class Inner2 extends Thread implements Runnable, Inner<Integer> {
        @Override
        public Integer element() {
            return 0;
        }
    }
}
