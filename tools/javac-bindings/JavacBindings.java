// The javac oracle of IDX-10's differential test: compiles every .java file under a project
// directory with the running JDK's javac (attribution only, no class files, no annotation
// processing) and writes, for every name javac binds, where it binds it. Run by
// `just javac-bindings` with the pinned JDK, in source-launcher mode; never edited output.
//
// Usage: java JavacBindings.java write|check OUT.tsv PROJECT_DIR
//
// Output (UTF-8, LF, tab-separated, sorted), byte offsets of the files as written:
//   # javac-bindings 1 ...                     a header line
//   file  PATH  FNV1A64                        each compiled file (PATH relative to PROJECT_DIR)
//   site  PATH  START  END  decl  KIND  NAME  DPATH  DSTART  DEND
//                                              bound to a declaration in the compiled files:
//                                              its kind, simple name and the bytes of its tree
//   site  PATH  START  END  decl  IMPLICIT_CONSTRUCTOR  NAME  DPATH  DSTART  DEND
//                                              the default constructor javac gives a class
//                                              that declares none: the class (NAME, its tree);
//                                              a record's implicit accessor is recorded as its
//                                              component (KIND RECORD_COMPONENT)
//   site  PATH  START  END  outside  KIND  QUALIFIED
//                                              bound to a declaration javac read from elsewhere
//   site  PATH  START  END  local  KIND  NAME   a local, parameter, pattern or type variable
//   site  PATH  START  END  package  NAME      a package
// `check` compares with OUT.tsv and fails on any difference.

import com.sun.source.tree.AnnotatedTypeTree;
import com.sun.source.tree.CompilationUnitTree;
import com.sun.source.tree.IdentifierTree;
import com.sun.source.tree.MemberSelectTree;
import com.sun.source.tree.NewClassTree;
import com.sun.source.tree.ParameterizedTypeTree;
import com.sun.source.tree.Tree;
import com.sun.source.util.JavacTask;
import com.sun.source.util.SourcePositions;
import com.sun.source.util.TreePath;
import com.sun.source.util.TreePathScanner;
import com.sun.source.util.Trees;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.TreeMap;
import java.util.stream.Stream;
import javax.lang.model.element.Element;
import javax.lang.model.element.ElementKind;
import javax.lang.model.element.ExecutableElement;
import javax.lang.model.element.PackageElement;
import javax.lang.model.element.TypeElement;
import javax.lang.model.util.Elements;
import javax.tools.Diagnostic;
import javax.tools.DiagnosticCollector;
import javax.tools.JavaCompiler;
import javax.tools.JavaFileObject;
import javax.tools.StandardJavaFileManager;
import javax.tools.ToolProvider;

public final class JavacBindings {
  private JavacBindings() {}

  /** A compiled file: its path relative to the project and the byte offset of each char. */
  record Source(String path, String text, int[] byteAt, long digest) {}

  public static void main(String[] args) throws IOException {
    if (args.length != 3 || !(args[0].equals("write") || args[0].equals("check"))) {
      System.err.println("usage: JavacBindings write|check OUT.tsv PROJECT_DIR");
      System.exit(2);
    }
    Path out = Path.of(args[1]);
    Path project = Path.of(args[2]).toAbsolutePath().normalize();
    List<Path> files;
    try (Stream<Path> walk = Files.walk(project)) {
      files = walk.filter(p -> p.toString().endsWith(".java")).sorted().toList();
    }
    JavaCompiler javac = ToolProvider.getSystemJavaCompiler();
    DiagnosticCollector<JavaFileObject> diagnostics = new DiagnosticCollector<>();
    StandardJavaFileManager manager =
        javac.getStandardFileManager(diagnostics, Locale.ROOT, StandardCharsets.UTF_8);
    JavacTask task =
        (JavacTask)
            javac.getTask(
                null,
                manager,
                diagnostics,
                List.of("-proc:none", "-implicit:none", "-Xlint:none", "--release", "21"),
                null,
                manager.getJavaFileObjectsFromPaths(files));
    List<CompilationUnitTree> units = new ArrayList<>();
    task.parse().forEach(units::add);
    task.analyze();
    boolean failed = false;
    for (Diagnostic<? extends JavaFileObject> d : diagnostics.getDiagnostics()) {
      if (d.getKind() == Diagnostic.Kind.ERROR) {
        System.err.println(d);
        failed = true;
      }
    }
    if (failed) {
      System.exit(1);
    }
    Trees trees = Trees.instance(task);
    Map<CompilationUnitTree, Source> sources = new HashMap<>();
    for (CompilationUnitTree unit : units) {
      Path file = Path.of(unit.getSourceFile().toUri()).toAbsolutePath().normalize();
      byte[] bytes = Files.readAllBytes(file);
      String text = new String(bytes, StandardCharsets.UTF_8);
      int[] byteAt = new int[text.length() + 1];
      int b = 0;
      for (int i = 0; i < text.length(); i++) {
        byteAt[i] = b;
        int c = text.charAt(i);
        if (Character.isHighSurrogate((char) c) && i + 1 < text.length()) {
          byteAt[i + 1] = b;
          b += 4;
          i++;
          continue;
        }
        b += c < 0x80 ? 1 : c < 0x800 ? 2 : 3;
      }
      byteAt[text.length()] = b;
      String relative = project.relativize(file).toString().replace('\\', '/');
      sources.put(unit, new Source(relative, text, byteAt, fnv1a64(bytes)));
    }
    TreeMap<String, String> lines = new TreeMap<>();
    for (CompilationUnitTree unit : units) {
      Source source = sources.get(unit);
      lines.put(
          source.path() + "\t", "file\t" + source.path() + "\t" + Long.toHexString(source.digest()));
      new Scanner(trees, task.getElements(), unit, source, sources, lines).scan(unit, null);
    }
    StringBuilder text = new StringBuilder();
    text.append("# javac-bindings 1: javac's binding of every name in the project;")
        .append(" written by `just javac-bindings`, never by hand\n");
    for (String line : lines.values()) {
      text.append(line).append('\n');
    }
    String written = text.toString();
    if (args[0].equals("write")) {
      Files.writeString(out, written, StandardCharsets.UTF_8);
      return;
    }
    String committed = Files.exists(out) ? Files.readString(out, StandardCharsets.UTF_8) : "";
    if (!committed.equals(written)) {
      System.err.println(
          out + " is stale: javac binds the project differently; run `just javac-bindings write`");
      System.exit(1);
    }
  }

  static long fnv1a64(byte[] bytes) {
    long hash = 0xcbf29ce484222325L;
    for (byte b : bytes) {
      hash ^= b & 0xff;
      hash *= 0x100000001b3L;
    }
    return hash;
  }

  /** Records the binding of every identifier, member select and instance creation. */
  static final class Scanner extends TreePathScanner<Void, Void> {
    private final Trees trees;
    private final Elements elements;
    private final CompilationUnitTree unit;
    private final Source source;
    private final Map<CompilationUnitTree, Source> sources;
    private final TreeMap<String, String> lines;
    private final SourcePositions positions;

    Scanner(
        Trees trees,
        Elements elements,
        CompilationUnitTree unit,
        Source source,
        Map<CompilationUnitTree, Source> sources,
        TreeMap<String, String> lines) {
      this.trees = trees;
      this.elements = elements;
      this.unit = unit;
      this.source = source;
      this.sources = sources;
      this.lines = lines;
      this.positions = trees.getSourcePositions();
    }

    @Override
    public Void visitIdentifier(IdentifierTree tree, Void unused) {
      int start = (int) positions.getStartPosition(unit, tree);
      record(start, tree.getName().toString(), trees.getElement(getCurrentPath()), false);
      return super.visitIdentifier(tree, unused);
    }

    @Override
    public Void visitMemberSelect(MemberSelectTree tree, Void unused) {
      int end = (int) positions.getEndPosition(unit, tree);
      if (end >= 0) {
        String name = tree.getIdentifier().toString();
        record(end - name.length(), name, trees.getElement(getCurrentPath()), false);
      }
      return super.visitMemberSelect(tree, unused);
    }

    @Override
    public Void visitNewClass(NewClassTree tree, Void unused) {
      Void result = super.visitNewClass(tree, unused);
      // The site of `new T(…)` is T's simple name; javac binds it to a constructor (of the
      // anonymous class, for a class body: recorded as is, so it never matches T's).
      Tree named = tree.getIdentifier();
      while (true) {
        if (named instanceof ParameterizedTypeTree p) {
          named = p.getType();
        } else if (named instanceof AnnotatedTypeTree a) {
          named = a.getUnderlyingType();
        } else {
          break;
        }
      }
      int end = (int) positions.getEndPosition(unit, named);
      String name =
          named instanceof IdentifierTree i
              ? i.getName().toString()
              : named instanceof MemberSelectTree m ? m.getIdentifier().toString() : null;
      if (name != null && end >= 0) {
        record(end - name.length(), name, trees.getElement(getCurrentPath()), true);
      }
      return result;
    }

    /** The binding of `name` written at char `start`; a tree javac made up (an implicit
     * constructor's `super()`, at its class's position) is not written there and is skipped. */
    private void record(int start, String name, Element element, boolean replace) {
      int end = start + name.length();
      if (element == null
          || start < 0
          || end > source.byteAt().length - 1
          || !source.text().startsWith(name, start)) {
        return;
      }
      int from = source.byteAt()[start];
      int to = source.byteAt()[end];
      String key = source.path() + "\t" + String.format("%08d\t%08d", from, to);
      if (!replace && lines.containsKey(key)) {
        return;
      }
      lines.put(
          key,
          "site\t" + source.path() + "\t" + from + "\t" + to + "\t" + describe(element));
    }

    private String describe(Element element) {
      ElementKind kind = element.getKind();
      if (element instanceof PackageElement p) {
        return "package\t" + p.getQualifiedName();
      }
      switch (kind) {
        case LOCAL_VARIABLE, PARAMETER, EXCEPTION_PARAMETER, RESOURCE_VARIABLE,
            BINDING_VARIABLE, TYPE_PARAMETER -> {
          return "local\t" + kind + "\t" + element.getSimpleName();
        }
        default -> {}
      }
      String described = declaration(element, kind.toString(), element.getSimpleName().toString());
      if (described != null) {
        return described;
      }
      if (kind == ElementKind.CONSTRUCTOR
          && elements.getOrigin(element) == Elements.Origin.MANDATED) {
        Element type = element.getEnclosingElement();
        described =
            declaration(type, "IMPLICIT_CONSTRUCTOR", type.getSimpleName().toString());
        if (described != null) {
          return described;
        }
      }
      if (kind == ElementKind.METHOD
          && element instanceof ExecutableElement accessor
          && accessor.getParameters().isEmpty()
          && element.getEnclosingElement() instanceof TypeElement record
          && record.getKind() == ElementKind.RECORD) {
        // A record's implicit accessor (no tree of its own, so not found above): its
        // component, the declaration a reader sees.
        for (Element component : record.getEnclosedElements()) {
          // The component's private field carries its tree (the component itself has none).
          if (component.getKind() == ElementKind.FIELD
              && component.getSimpleName().equals(element.getSimpleName())) {
            described =
                declaration(component, "RECORD_COMPONENT", component.getSimpleName().toString());
            if (described != null) {
              return described;
            }
          }
        }
      }
      return "outside\t" + kind + "\t" + qualified(element);
    }

    /** `element` as a declaration of the compiled files, or null when it has no tree there. */
    private String declaration(Element element, String kind, String name) {
      TreePath path = trees.getPath(element);
      if (path != null && sources.containsKey(path.getCompilationUnit())) {
        CompilationUnitTree at = path.getCompilationUnit();
        Source declared = sources.get(at);
        long start = positions.getStartPosition(at, path.getLeaf());
        long end = positions.getEndPosition(at, path.getLeaf());
        if (start >= 0 && end >= start && end <= declared.byteAt().length - 1) {
          return "decl\t"
              + kind
              + "\t"
              + name
              + "\t"
              + declared.path()
              + "\t"
              + declared.byteAt()[(int) start]
              + "\t"
              + declared.byteAt()[(int) end];
        }
      }
      return null;
    }

    private static String qualified(Element element) {
      if (element instanceof TypeElement t) {
        return t.getQualifiedName().toString();
      }
      Element owner = element.getEnclosingElement();
      String prefix =
          owner instanceof TypeElement t ? t.getQualifiedName().toString() : String.valueOf(owner);
      return prefix + "#" + element.getSimpleName();
    }
  }
}
