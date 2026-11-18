package grammar;

import java.lang.annotation.ElementType;
import java.lang.annotation.Target;
import java.util.List;
import java.util.Map;

/** Annotations in every position: declarations, type uses, type parameters, varargs, arrays. */
@Target({ElementType.TYPE, ElementType.METHOD, ElementType.FIELD, ElementType.PARAMETER,
        ElementType.LOCAL_VARIABLE, ElementType.CONSTRUCTOR})
@interface Mark {
    String value() default "";
}

/** A type-use annotation. */
@Target({ElementType.TYPE_USE, ElementType.TYPE_PARAMETER})
@interface Use {}

@Mark("type")
class AnnotationsEverywhere<@Use T extends @Use Comparable<T>> {
    @Mark private @Use List<@Use String> names;
    private int @Use [] counts;
    private Map<@Use String, @Use List<@Use ? extends Number>> index;

    @Mark("ctor")
    AnnotationsEverywhere() {}

    @Mark
    <@Use U> @Use String join(@Use AnnotationsEverywhere<T> this, @Mark U first, String @Use ... rest)
            throws @Use IllegalStateException {
        @Mark String local = String.valueOf(first);
        Object cast = (@Use Object) local;
        return local + rest.length + cast;
    }

    String @Use [] array(int @Use [] @Use [] grid) {
        return new String @Use [grid.length];
    }
}
