package syntax;

/**
 * The type's Javadoc.
 */
@SuppressWarnings("all")
class Javadoc {
    /** The field's Javadoc, shared by both declarators. */
    int first, second;

    /** Attached although a line comment follows: other comments may sit between. */
    // a note
    void withNote() {}

    /* A block comment, not Javadoc. */
    void plain() {}

    /** Javadoc. */

    // Blank lines between are still white space.
    void spaced() {}

    @Deprecated
    /** After an annotation: inside the declaration, so not its Javadoc. */
    void late() {}

    /**/
    void empty() {}
}
