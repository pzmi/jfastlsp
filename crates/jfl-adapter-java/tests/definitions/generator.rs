//! A seeded generator of nested Java constructs for the javac differential (IDX-10): each case
//! file puts the same uses (`val`, `helper()`, `this.val`, `this.helper()`, `Item`, `new
//! Item()`) behind a random chain of wrappers that change what they bind to, or only look as if
//! they did: anonymous classes (in an argument, behind a nested `new`, a `[` or a lambda, in an
//! array initialiser), lambdas, local classes, enum constant bodies, `switch` rules, locals,
//! pattern and `for` variables named like a field, comments and strings holding braces,
//! annotations whose strings spell a name, member and static nested classes, a generic class
//! whose type parameter is named like a type, class headers that name a member type. javac
//! compiles the output (`just javac-bindings`) and records where each name binds.
//!
//! Deterministic: the same seed gives the same files byte for byte, which
//! `tests/definitions/main.rs::should_regenerate_the_committed_corpus_when_the_seed_is_kept` pins.

use std::fmt::Write as _;

/// The generator's seed: changing it regenerates the corpus (and its bindings).
pub(crate) const SEED: u64 = 0x1D10_2026_1119_0002;

/// How many case files the corpus holds.
pub(crate) const CASES: usize = 32;

/// SplitMix64: a small, well-mixed generator with no dependency.
pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `n` (n > 0).
    pub(crate) fn below(&mut self, n: u64) -> u64 {
        self.next().checked_rem(n).unwrap_or(0)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

/// What the code at a point can use.
#[derive(Debug, Clone, Copy)]
struct Context {
    /// `this` has a field `val` (declared or inherited).
    this_val: bool,
    /// `this` has a method `helper()`.
    this_helper: bool,
    /// `Item` names a type variable here.
    item_is_variable: bool,
    /// A local `val` is in scope (a lambda does not end it).
    local_val: bool,
}

/// The files shared by every case, by path under the project.
const FIXED: [(&str, &str); 6] = [
    (
        "src/main/java/gen/Base.java",
        "package gen;\n\npublic abstract class Base {\n    protected int val = 1;\n\n    public Base() {\n    }\n\n    public Base(Object o) {\n    }\n\n    public Base(Runnable r) {\n    }\n\n    public int helper() {\n        return 1;\n    }\n\n    public static class Item {\n    }\n\n    public abstract void run();\n}\n",
    ),
    (
        "src/main/java/gen/Item.java",
        "package gen;\n\npublic class Item {\n    public Item() {\n    }\n\n    public Item(int x) {\n    }\n}\n",
    ),
    (
        "src/main/java/gen/Constants.java",
        "package gen;\n\npublic interface Constants {\n    int val = 10;\n}\n",
    ),
    (
        "src/main/java/gen/other/PBase2.java",
        "package gen.other;\n\npublic class PBase2 {\n    public int z;\n\n    public static class Gadget {\n    }\n}\n",
    ),
    (
        "src/main/java/gen/other/PBase1.java",
        "package gen.other;\n\npublic class PBase1 extends PBase2 {\n    int z;\n\n    static class Gadget {\n    }\n}\n",
    ),
    (
        "src/main/java/gen/other/Item.java",
        "package gen.other;\n\npublic class Item {\n    public Item() {\n    }\n}\n",
    ),
];

/// Every file of the corpus: its path under the project and its text.
pub(crate) fn generate(seed: u64) -> Vec<(String, String)> {
    let mut rng = Rng(seed);
    let mut files: Vec<(String, String)> = FIXED
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
        .collect();
    for case in 0..CASES {
        let name = format!("Case{case:02}");
        let text = case_file(&mut rng, &name);
        files.push((format!("src/main/java/gen/{name}.java"), text));
    }
    files
}

/// One case file: a top-level class with a few hosts, each holding a chain of wrappers around
/// the uses.
fn case_file(rng: &mut Rng, name: &str) -> String {
    let mut out = String::from("package gen;\n\n");
    if rng.chance(40) {
        out.push_str("import gen.other.Item;\n\n");
    }
    let parent = rng.below(3);
    let header = match parent {
        0 => String::new(),
        1 => " extends Base".to_owned(),
        _ => " implements Constants".to_owned(),
    };
    let _ = writeln!(out, "public class {name}{header} {{");
    // `val` and `helper()` must be there for every use; an inherited one may be hidden.
    if parent == 0 || rng.chance(50) {
        out.push_str("    int val = 2;\n\n");
    }
    out.push_str("    public int helper() {\n        return 2;\n    }\n\n");
    if rng.chance(30) {
        out.push_str("    public int helper(int x) {\n        return x;\n    }\n\n");
    }
    if rng.chance(30) {
        out.push_str("    static class Item {\n    }\n\n");
    }
    if parent == 1 {
        out.push_str("    public void run() {\n    }\n\n");
    }
    out.push_str("    static void consume(Object o) {\n    }\n\n");
    // Names a library supertype also declares (see the `Lib` host), names a hidden member
    // declares (the hiding hosts), and a field named like the class (the `Q` host).
    out.push_str("    static class Entry<K, V> {\n    }\n\n    int modCount;\n\n    public int size() {\n        return 0;\n    }\n\n");
    let _ = write!(
        out,
        "    int z;\n\n    static class Gadget {{\n    }}\n\n    int k;\n\n    int y;\n\n    static class Thing {{\n    }}\n\n    int {name};\n\n"
    );
    let mut counter = 0_u32;
    let hosts = rng.below(3).saturating_add(2);
    for host in 0..hosts {
        let host_kind = rng.below(14);
        let body = |rng: &mut Rng, counter: &mut u32, context: Context, indent: usize| {
            let mut text = String::new();
            chain(rng, counter, context, indent, &mut text);
            text
        };
        let top = Context {
            this_val: true,
            this_helper: true,
            item_is_variable: false,
            local_val: false,
        };
        match host_kind {
            0 | 1 => {
                let inner = body(rng, &mut counter, top, 2);
                let _ = writeln!(out, "    void probe{host}() {{\n{inner}    }}\n");
            }
            2 => {
                // A member class, maybe with its own supertype and members.
                let extends = rng.below(3);
                let header = match extends {
                    0 => "",
                    1 => " extends Base",
                    _ => " implements Constants",
                };
                let _ = writeln!(out, "    class In{host}{header} {{");
                let own_val = rng.chance(50);
                if own_val {
                    out.push_str("        int val = 3;\n\n");
                }
                let own_helper = rng.chance(50);
                if own_helper {
                    out.push_str(
                        "        public int helper() {\n            return 3;\n        }\n\n",
                    );
                }
                if rng.chance(30) {
                    out.push_str("        static class Item {\n        }\n\n");
                }
                if extends == 1 {
                    out.push_str("        public void run() {\n        }\n\n");
                }
                // `this` is the member class: what it declares or inherits.
                let context = Context {
                    this_val: own_val || extends != 0,
                    this_helper: own_helper || extends == 1,
                    ..top
                };
                let inner = body(rng, &mut counter, context, 3);
                let _ = write!(
                    out,
                    "        void probe() {{\n{inner}        }}\n    }}\n\n"
                );
            }
            3 => {
                // A static nested class: no outer instance, so it declares what it uses.
                let extends = rng.chance(50);
                let _ = writeln!(
                    out,
                    "    static class St{host}{} {{",
                    if extends { " extends Base" } else { "" }
                );
                out.push_str("        int val = 4;\n\n        public int helper() {\n            return 4;\n        }\n\n");
                if extends {
                    out.push_str("        public void run() {\n        }\n\n");
                }
                let inner = body(rng, &mut counter, top, 3);
                let _ = write!(
                    out,
                    "        void probe() {{\n{inner}        }}\n    }}\n\n"
                );
            }
            4 => {
                // An enum whose first constant has a body: an anonymous class.
                let inner = body(rng, &mut counter, top, 4);
                let _ = write!(
                    out,
                    "    enum En{host} {{\n        A {{\n            void probe() {{\n{inner}            }}\n        }},\n        B;\n\n        int val = 5;\n\n        public int helper() {{\n            return 5;\n        }}\n\n        void probe() {{\n        }}\n    }}\n\n"
                );
            }
            5 => {
                // A generic member class whose type parameter is named `Item`, with an
                // annotation whose string spells the class's name (the second review's N5).
                let context = Context {
                    item_is_variable: true,
                    this_val: false,
                    this_helper: false,
                    ..top
                };
                let inner = body(rng, &mut counter, context, 3);
                let _ = write!(
                    out,
                    "    @SuppressWarnings(\"Gen{host}\")\n    class Gen{host}<Item> {{\n        void probe() {{\n{inner}        }}\n    }}\n\n"
                );
            }
            7 => {
                // A member class whose supertype is a library class: `size()`, `modCount` and
                // `Entry` are its inherited members, outside the index, not the outer class's.
                let _ = write!(
                    out,
                    "    class Lib{host} extends java.util.HashMap<String, String> {{\n        void probe() {{\n            int s{host} = size() + modCount;\n            Entry<String, String> e{host} = null;\n            consume(e{host});\n        }}\n    }}\n\n"
                );
            }
            9 => {
                // A private field and member type of a superclass hide the public ones further
                // up, inherited or not: the lookup goes to the enclosing class (JLS 8.3, 8.5; the
                // slice-2 review's H1).
                let _ = write!(
                    out,
                    "    static class HTop{host} {{\n        int val;\n\n        static class Item {{\n        }}\n    }}\n\n    static class HMid{host} extends HTop{host} {{\n        private int val;\n\n        private static class Item {{\n        }}\n    }}\n\n    class HIn{host} extends HMid{host} {{\n        int f() {{\n            return val;\n        }}\n\n        Item g() {{\n            return null;\n        }}\n\n        Object n() {{\n            return new Item();\n        }}\n    }}\n\n"
                );
            }
            10 => {
                // Package-private members of a supertype in another package hide its public ones.
                let _ = write!(
                    out,
                    "    class PIn{host} extends gen.other.PBase1 {{\n        int f() {{\n            return z;\n        }}\n\n        Object g() {{\n            return new Gadget();\n        }}\n    }}\n\n"
                );
            }
            11 => {
                // A private constant hides an interface's; a second top-level class's private
                // members hide its superclass's.
                let _ = write!(
                    out,
                    "    interface HK{host} {{\n        int k = 1;\n    }}\n\n    static class HKBase{host} implements HK{host} {{\n        private static final int k = 2;\n    }}\n\n    class HKIn{host} extends HKBase{host} {{\n        int f() {{\n            return k;\n        }}\n    }}\n\n    class TIn{host} extends {name}Mid {{\n        int f() {{\n            return y;\n        }}\n\n        Thing g() {{\n            return null;\n        }}\n    }}\n\n"
                );
            }
            12 => {
                // `Name.this` against a field named like the class (H2), and a record's
                // implicit field and accessor.
                let _ = write!(
                    out,
                    "    class Q{host} {{\n        int val;\n\n        int f() {{\n            return {name}.this.val + {name}.this.helper();\n        }}\n    }}\n\n    record Rec{host}(int val) {{\n        int g() {{\n            return val() + val;\n        }}\n    }}\n\n"
                );
            }
            13 => {
                // Same-signature methods from two branches of a hierarchy, none overriding the
                // other: an abstract superclass method against an interface default, covariant
                // returns across a class and an interface or two interfaces, identical abstract
                // methods (the slice-2 review's third pass; javac picks by JLS 15.12.2.5).
                let _ = write!(
                    out,
                    "    interface AdI{host} {{\n        default int m() {{\n            return 1;\n        }}\n    }}\n\n    abstract static class AdB{host} {{\n        abstract int m();\n    }}\n\n    abstract static class AbsDef{host} extends AdB{host} implements AdI{host} {{\n        int f() {{\n            return m();\n        }}\n\n        int g() {{\n            return this.m();\n        }}\n    }}\n\n    interface AaI{host} {{\n        String m();\n    }}\n\n    abstract static class AaB{host} {{\n        abstract Object m();\n    }}\n\n    abstract static class AbsAbs{host} extends AaB{host} implements AaI{host} {{\n        Object f() {{\n            return m();\n        }}\n    }}\n\n    interface Cov1{host} {{\n        Object m();\n    }}\n\n    interface Cov2{host} {{\n        String m();\n    }}\n\n    abstract static class CovI{host} implements Cov1{host}, Cov2{host} {{\n        Object f() {{\n            return m();\n        }}\n\n        Object g() {{\n            return this.m();\n        }}\n    }}\n\n    interface CovJ{host} extends Cov1{host}, Cov2{host} {{\n        default Object h() {{\n            return m();\n        }}\n    }}\n\n    interface SaI{host} {{\n        int m();\n    }}\n\n    interface SaJ{host} {{\n        int m();\n    }}\n\n    abstract static class SaB{host} {{\n        abstract int m();\n    }}\n\n    abstract static class SameAbs{host} extends SaB{host} implements SaI{host} {{\n        int f() {{\n            return m();\n        }}\n    }}\n\n    abstract static class SameAbs2{host} implements SaI{host}, SaJ{host} {{\n        int f() {{\n            return m();\n        }}\n    }}\n\n"
                );
            }
            _ => {
                // Class headers and a type parameter's bound naming `Item`, beside a member type
                // of that name (N3), and a method's bound, which sees it.
                let _ = write!(
                    out,
                    "    static class Hd{host} extends Item {{\n        static class Item {{\n        }}\n\n        Item own;\n    }}\n\n    @SuppressWarnings(\"Item\")\n    static class Hb{host} implements Comparable<Hb{host}> {{\n        public int compareTo(Hb{host} other) {{\n            return 0;\n        }}\n    }}\n\n    static class Bd{host}<T extends Item> {{\n        static class Item {{\n        }}\n\n        <U extends Item> U pick(T t, U u) {{\n            return u;\n        }}\n    }}\n\n"
                );
            }
        }
    }
    out.push_str("}\n");
    // A second and third top-level class in the file, for the `TIn` host.
    let _ = write!(
        out,
        "\nclass {name}Top {{\n    int y;\n\n    static class Thing {{\n    }}\n}}\n\nclass {name}Mid extends {name}Top {{\n    private int y;\n\n    private static class Thing {{\n    }}\n}}\n"
    );
    out
}

fn pad(indent: usize) -> String {
    "    ".repeat(indent)
}

/// A chain of zero to three wrappers around the uses, at `indent`.
fn chain(rng: &mut Rng, counter: &mut u32, context: Context, indent: usize, out: &mut String) {
    let depth = rng.below(4);
    wrap(rng, counter, context, indent, depth, out);
}

fn next(counter: &mut u32) -> u32 {
    *counter = counter.saturating_add(1);
    *counter
}

/// `depth` wrappers, then the uses.
fn wrap(
    rng: &mut Rng,
    counter: &mut u32,
    context: Context,
    indent: usize,
    depth: u64,
    out: &mut String,
) {
    let p = pad(indent);
    if depth == 0 {
        uses(rng, counter, context, indent, out);
        return;
    }
    let n = next(counter);
    let deeper = depth.saturating_sub(1);
    let anonymous = Context {
        this_val: true,
        this_helper: true,
        local_val: false,
        ..context
    };
    let inner = |rng: &mut Rng, counter: &mut u32, context: Context, indent: usize| {
        let mut text = String::new();
        wrap(rng, counter, context, indent, deeper, &mut text);
        text
    };
    match rng.below(17) {
        0 => {
            let body = inner(rng, counter, context, indent.saturating_add(1));
            let _ = writeln!(out, "{p}{{\n{body}{p}}}");
        }
        1 => {
            let body = inner(rng, counter, context, indent.saturating_add(1));
            let _ = writeln!(out, "{p}int n{n} = 1;\n{p}if (n{n} > 0) {{\n{body}{p}}}");
        }
        2 => {
            let body = inner(rng, counter, context, indent.saturating_add(1));
            let _ = write!(
                out,
                "{p}for (int i{n} = 0; i{n} < 1; i{n}++) {{\n{body}{p}}}\n"
            );
        }
        3 => {
            let body = inner(rng, counter, context, indent.saturating_add(1));
            let _ = write!(
                out,
                "{p}try {{\n{body}{p}}} catch (RuntimeException e{n}) {{\n{p}}}\n"
            );
        }
        4 => {
            let body = inner(rng, counter, context, indent);
            let _ = write!(
                out,
                "{p}/* {{ new Base() {{ int val; */\n{p}// }} helper() {{\n{body}"
            );
        }
        5 => {
            let body = inner(rng, counter, context, indent);
            let _ = write!(
                out,
                "{p}String s{n} = \"}} {{ new Base() {{ val helper()\";\n{body}"
            );
        }
        6 => {
            let body = inner(rng, counter, context, indent);
            let _ = write!(out, "{p}@SuppressWarnings(\"val\") int a{n} = 0;\n{body}");
        }
        7 => {
            let body = inner(rng, counter, context, indent.saturating_add(1));
            let _ = writeln!(out, "{p}Runnable r{n} = () -> {{\n{body}{p}}};");
        }
        8 => {
            let own = rng.chance(50);
            let ctx = anonymous;
            let body = inner(rng, counter, ctx, indent.saturating_add(2));
            let _ = writeln!(out, "{p}consume(new Base() {{");
            if own {
                let _ = writeln!(out, "{p}    int val = 6;\n");
            }
            let _ = write!(
                out,
                "{p}    public void run() {{\n{body}{p}    }}\n{p}}});\n"
            );
        }
        9 => {
            // An anonymous class behind a nested `new`, a `[` or a lambda block (N1).
            let argument = match rng.below(3) {
                0 => "new Object()".to_owned(),
                1 => format!("xs{n}[0]"),
                _ => "() -> { }".to_owned(),
            };
            let body = inner(rng, counter, anonymous, indent.saturating_add(2));
            let _ = write!(
                out,
                "{p}Object[] xs{n} = {{ null }};\n{p}Base b{n} = new Base({argument}) {{\n{p}    public int helper() {{\n{p}        return 7;\n{p}    }}\n\n{p}    public void run() {{\n{body}{p}    }}\n{p}}};\n"
            );
        }
        10 => {
            let body = inner(rng, counter, anonymous, indent.saturating_add(3));
            let _ = write!(
                out,
                "{p}Object[] arr{n} = {{\n{p}    new Base() {{\n{p}        public void run() {{\n{body}{p}        }}\n{p}    }}\n{p}}};\n"
            );
        }
        11 => {
            let own = rng.chance(50);
            let body = inner(rng, counter, anonymous, indent.saturating_add(2));
            let _ = writeln!(out, "{p}class Loc{n} extends Base {{");
            if own {
                let _ = writeln!(out, "{p}    int val = 8;\n");
            }
            let _ = write!(
                out,
                "{p}    public void run() {{\n{body}{p}    }}\n{p}}}\n{p}new Loc{n}().run();\n"
            );
        }
        12 => {
            let body = inner(rng, counter, context, indent.saturating_add(2));
            let _ = write!(
                out,
                "{p}int n{n} = 1;\n{p}switch (n{n}) {{\n{p}    case 1 -> {{\n{body}{p}    }}\n{p}    default -> {{\n{p}    }}\n{p}}}\n"
            );
        }
        13 if !context.local_val => {
            let ctx = Context {
                local_val: true,
                ..context
            };
            let body = inner(rng, counter, ctx, indent);
            let _ = write!(out, "{p}int val = 9;\n{body}");
        }
        14 if !context.local_val => {
            let ctx = Context {
                local_val: true,
                ..context
            };
            let body = inner(rng, counter, ctx, indent.saturating_add(1));
            let _ = write!(
                out,
                "{p}Object o{n} = 1;\n{p}if (o{n} instanceof Integer val) {{\n{body}{p}}}\n"
            );
        }
        15 if !context.local_val => {
            let ctx = Context {
                local_val: true,
                ..context
            };
            let body = inner(rng, counter, ctx, indent.saturating_add(1));
            let _ = writeln!(out, "{p}for (int val : new int[] {{1}}) {{\n{body}{p}}}");
        }
        _ => {
            // A ternary and a parenthesised argument around a call.
            let body = inner(rng, counter, context, indent);
            let _ = write!(out, "{p}int t{n} = (true ? 1 : 0) + (2);\n{body}");
        }
    }
}

/// The uses at the end of a chain.
fn uses(rng: &mut Rng, counter: &mut u32, context: Context, indent: usize, out: &mut String) {
    let p = pad(indent);
    let n = next(counter);
    let _ = writeln!(out, "{p}int v{n} = val + 0;");
    let _ = writeln!(out, "{p}int h{n} = helper();");
    if context.this_val && rng.chance(70) {
        let _ = writeln!(out, "{p}int tv{n} = this.val;");
    }
    if context.this_helper && rng.chance(70) {
        let _ = writeln!(out, "{p}int th{n} = this.helper();");
    }
    let _ = writeln!(out, "{p}Item it{n} = null;");
    if !context.item_is_variable {
        let _ = writeln!(out, "{p}Object ni{n} = new Item();");
    }
    let _ = writeln!(out, "{p}consume(it{n});");
}
