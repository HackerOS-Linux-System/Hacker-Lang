import com.google.gson.Gson;
import com.google.gson.reflect.TypeToken;
import picocli.CommandLine;
import picocli.CommandLine.Command;
import picocli.CommandLine.Option;
import picocli.CommandLine.Parameters;
import org.jline.reader.EndOfFileException;
import org.jline.reader.LineReader;
import org.jline.reader.LineReaderBuilder;
import org.jline.reader.UserInterruptException;
import org.jline.reader.impl.history.DefaultHistory;
import org.jline.terminal.Terminal;
import org.jline.terminal.TerminalBuilder;
import org.yaml.snakeyaml.Yaml;
import java.io.*;
import java.lang.reflect.Type;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.attribute.PosixFilePermission;
import java.util.*;
import java.util.concurrent.Callable;

@Command(
    name = "hackerc",
    version = "0.0.9",
    description = "Hacker Lang CLI – advanced scripting for Debian-based systems",
    subcommands = {
        HackerCLI.Run.class,
        HackerCLI.Compile.class,
        HackerCLI.Check.class,
        HackerCLI.Init.class,
        HackerCLI.Clean.class,
        HackerCLI.Repl.class,
        HackerCLI.Editor.class,
        HackerCLI.Unpack.class,
        HackerCLI.Version.class,
        HackerCLI.HelpUI.class,
        HackerCLI.Help.class
    }
)
class HackerCLI implements Callable<Integer> {
    @Option(names = {"-h", "--help"}, usageHelp = true, description = "Show this help message and exit.")
    boolean helpRequested;

    @Option(names = {"-V", "--version"}, versionHelp = true, description = "Print version information and exit.")
    boolean versionRequested;

    private static final String VERSION = "0.0.9";
    private static final String HACKER_DIR = "~/.hackeros/hacker-lang";
    private static final String BIN_DIR = HACKER_DIR + "/bin";
    private static final String HISTORY_FILE = "~/.hackeros/history/hacker_repl_history";
    private static final Gson gson = new Gson();

    // ---------- Kolory (ładniejsze niż w Go) ----------
    private static final String RESET = "\u001B[0m";
    private static final String BOLD = "\u001B[1m";
    private static final String CYAN = "\u001B[96m";
    private static final String PURPLE = "\u001B[95m";
    private static final String GREEN = "\u001B[92m";
    private static final String RED = "\u001B[91m";
    private static final String YELLOW = "\u001B[93m";
    private static final String BLUE = "\u001B[94m";
    private static final String WHITE = "\u001B[97m";

    private static String expand(String p) {
        if (p.startsWith("~")) {
            p = System.getProperty("user.home") + p.substring(1);
        }
        return p;
    }

    private static void ensureDirs() throws IOException {
        Files.createDirectories(Paths.get(expand(BIN_DIR)));
        Files.createDirectories(Paths.get(expand(HACKER_DIR + "/libs")));
        Files.createDirectories(Paths.get(expand(HISTORY_FILE)).getParent());
    }

    // ---------- Parsowanie wyjścia parsera ----------
    private record ParseResult(
            List<String> deps, List<String> libs, Map<String, String> vars,
            List<String> cmds, List<String> includes, List<String> binaries,
            List<String> plugins, List<String> errors, Map<String, String> config) {}

    private static ParseResult runParser(String file, boolean verbose) throws Exception {
        String parser = expand(BIN_DIR + "/hacker-parser");
        List<String> cmd = new ArrayList<>(List.of(parser, file));
        if (verbose) cmd.add("--verbose");
        ProcessBuilder pb = new ProcessBuilder(cmd);
        Process p = pb.start();
        String output = new String(p.getInputStream().readAllBytes());
        String error = new String(p.getErrorStream().readAllBytes());
        int code = p.waitFor();
        if (code != 0) {
            System.out.println(RED + BOLD + "Parser error: " + error.trim() + RESET);
            throw new RuntimeException("Parser failed");
        }
        Type type = new TypeToken<Map<String, Object>>(){}.getType();
        Map<String, Object> map = gson.fromJson(output, type);
        List<String> deps = getList(map, "deps");
        List<String> libs = getList(map, "libs");
        List<String> cmds = getList(map, "cmds");
        List<String> includes = getList(map, "includes");
        List<String> binaries = getList(map, "binaries");
        List<String> plugins = getList(map, "plugins");
        List<String> errors = getList(map, "errors");
        Map<String, String> vars = getMap(map, "vars");
        Map<String, String> config = getMap(map, "config");
        return new ParseResult(deps, libs, vars, cmds, includes, binaries, plugins, errors, config);
    }

    @SuppressWarnings("unchecked")
    private static List<String> getList(Map<String, Object> m, String key) {
        return m.containsKey(key) ? (List<String>) m.get(key) : new ArrayList<>();
    }

    @SuppressWarnings("unchecked")
    private static Map<String, String> getMap(Map<String, Object> m, String key) {
        return m.containsKey(key) ? (Map<String, String>) m.get(key) : new HashMap<>();
    }

    // ---------- Komendy ----------
    @Command(name = "run", description = "Execute .hacker script")
    public static class Run implements Callable<Integer> {
        public Run() {}

        @Parameters(index = "0") String file;
        @Option(names = "--verbose") boolean verbose;
        @Override public Integer call() throws Exception {
            System.out.println(CYAN + BOLD + "Running script..." + RESET);
            if (".".equals(file)) {
                runBytesProject(verbose);
            } else {
                executeScript(file, verbose);
            }
            return 0;
        }
    }

    @Command(name = "compile", description = "Compile to native binary")
    public static class Compile implements Callable<Integer> {
        public Compile() {}

        @Parameters(index = "0") String file;
        @Option(names = {"-o", "--output"}) String output;
        @Option(names = "--verbose") boolean verbose;
        @Option(names = "--bytes") boolean bytesMode;
        @Override public Integer call() throws Exception {
            System.out.println(CYAN + BOLD + "Compiling..." + RESET);
            if (bytesMode) {
                compileBytesProject(output != null ? output : "", verbose);
            } else {
                compileNormal(file, output, verbose);
            }
            return 0;
        }
    }

    @Command(name = "check", description = "Validate syntax")
    public static class Check implements Callable<Integer> {
        public Check() {}

        @Parameters(index = "0") String file;
        @Option(names = "--verbose") boolean verbose;
        @Override public Integer call() throws Exception {
            System.out.println(CYAN + BOLD + "Checking syntax..." + RESET);
            ParseResult r = runParser(file, verbose);
            if (!r.errors().isEmpty()) {
                System.out.println(RED + BOLD + "Syntax errors:" + RESET);
                r.errors().forEach(e -> System.out.println(RED + " - " + e + RESET));
                return 1;
            }
            System.out.println(GREEN + BOLD + "Syntax validation passed!" + RESET);
            return 0;
        }
    }

    @Command(name = "init", description = "Generate template script")
    public static class Init implements Callable<Integer> {
        public Init() {}

        @Parameters(index = "0") String file;
        @Option(names = "--verbose") boolean verbose;
        @Override public Integer call() throws Exception {
            System.out.println(CYAN + BOLD + "Initializing template..." + RESET);
            Path path = Paths.get(file);
            if (Files.exists(path)) {
                System.out.println(RED + BOLD + "File " + file + " already exists!" + RESET);
                return 1;
            }
            String template = """
! Hacker Lang advanced template
// sudo ! Privileged operations
// curl ! For downloads
# network-utils ! Custom library example
@APP_NAME=HackerApp ! Application name
@LOG_LEVEL=debug
=3 > echo "Iteration: $APP_NAME" ! Loop example
? [ -f /etc/os-release ] > cat /etc/os-release | grep PRETTY_NAME ! Conditional
& ping -c 1 google.com ! Background task
# logging ! Include logging library
> echo "Starting update..."
> sudo apt update && sudo apt upgrade -y ! System update
[
Author=Advanced User
Version=1.0
Description=System maintenance script
]
""";
            Files.writeString(path, template);
            System.out.println(GREEN + BOLD + "Initialized template at " + file + RESET);
            if (verbose) {
                System.out.println(YELLOW + template + RESET);
            }
            return 0;
        }
    }

    @Command(name = "clean", description = "Remove temporary files")
    public static class Clean implements Callable<Integer> {
        public Clean() {}

        @Option(names = "--verbose") boolean verbose;
        @Override public Integer call() throws Exception {
            System.out.println(CYAN + BOLD + "Cleaning temporary files..." + RESET);
            Path tempDir = Paths.get(System.getProperty("java.io.tmpdir"));
            int count = 0;
            try (var stream = Files.list(tempDir)) {
                var files = stream.filter(f -> f.getFileName().toString().startsWith("tmp") && f.toString().endsWith(".sh"))
                        .toList();
                for (Path f : files) {
                    Files.delete(f);
                    count++;
                    if (verbose) {
                        System.out.println(YELLOW + "Removed: " + f + RESET);
                    }
                }
            }
            System.out.println(GREEN + BOLD + "Removed " + count + " temporary files" + RESET);
            return 0;
        }
    }

    @Command(name = "repl", description = "Interactive REPL")
    public static class Repl implements Callable<Integer> {
        public Repl() {}

        @Option(names = "--verbose") boolean verbose;
        @Override public Integer call() throws Exception {
            System.out.println(CYAN + BOLD + "Starting REPL..." + RESET);
            startRepl(verbose);
            return 0;
        }
    }

    @Command(name = "editor", description = "Launch hacker-editor")
    public static class Editor implements Callable<Integer> {
        public Editor() {}

        @Parameters(arity = "0..1", description = "Optional file to edit") String file = "";
        @Override public Integer call() throws Exception {
            System.out.println(CYAN + BOLD + "Launching editor..." + RESET);
            String editorPath = expand(BIN_DIR + "/hacker-editor");
            List<String> cmd = new ArrayList<>(List.of(editorPath));
            if (!file.isEmpty()) cmd.add(file);
            ProcessBuilder pb = new ProcessBuilder(cmd);
            pb.inheritIO();
            int code = pb.start().waitFor();
            if (code != 0) {
                System.out.println(RED + BOLD + "Editor failed with code " + code + RESET);
                return 1;
            }
            System.out.println(GREEN + BOLD + "Editor session completed." + RESET);
            return 0;
        }
    }

    @Command(name = "unpack", description = "Unpack and install bytes")
    public static class Unpack implements Callable<Integer> {
        public Unpack() {}

        @Parameters(index = "0", description = "Target to unpack, e.g., bytes") String target;
        @Option(names = "--verbose") boolean verbose;
        @Override public Integer call() throws Exception {
            System.out.println(CYAN + BOLD + "Unpacking..." + RESET);
            if (!"bytes".equals(target)) {
                System.out.println(RED + BOLD + "Unknown unpack target: " + target + RESET);
                return 1;
            }
            String bytesPath1 = expand(HACKER_DIR + "/bin/bytes");
            String bytesPath2 = "/usr/bin/bytes";
            if (Files.exists(Paths.get(bytesPath1))) {
                System.out.println(GREEN + BOLD + "Bytes already installed at " + bytesPath1 + RESET);
                return 0;
            }
            if (Files.exists(Paths.get(bytesPath2))) {
                System.out.println(GREEN + BOLD + "Bytes already installed at " + bytesPath2 + RESET);
                return 0;
            }
            Files.createDirectories(Paths.get(new File(bytesPath1).getParent()));
            String url = "https://github.com/Bytes-Repository/Bytes-CLI-Tool/releases/download/v0.3/bytes";
            HttpClient client = HttpClient.newHttpClient();
            HttpRequest req = HttpRequest.newBuilder().uri(URI.create(url)).build();
            HttpResponse<InputStream> resp = client.send(req, HttpResponse.BodyHandlers.ofInputStream());
            if (resp.statusCode() != 200) {
                System.out.println(RED + BOLD + "Error: status code " + resp.statusCode() + RESET);
                return 1;
            }
            try (InputStream in = resp.body();
                 OutputStream out = Files.newOutputStream(Paths.get(bytesPath1))) {
                in.transferTo(out);
            }
            Files.setPosixFilePermissions(Paths.get(bytesPath1), Set.of(PosixFilePermission.OWNER_READ, PosixFilePermission.OWNER_WRITE, PosixFilePermission.OWNER_EXECUTE));
            if (verbose) {
                System.out.println(GREEN + "Downloaded from " + url + " to " + bytesPath1 + RESET);
            }
            System.out.println(GREEN + BOLD + "Bytes installed successfully!" + RESET);
            return 0;
        }
    }

    @Command(name = "version", description = "Display version")
    public static class Version implements Callable<Integer> {
        public Version() {}

        @Override public Integer call() {
            System.out.println(BLUE + BOLD + "Hacker Lang v" + VERSION + RESET);
            return 0;
        }
    }

    @Command(name = "help-ui", description = "Show special commands list")
    public static class HelpUI implements Callable<Integer> {
        public HelpUI() {}

        @Override public Integer call() throws Exception {
            System.out.println(CYAN + BOLD + "Launching Help UI..." + RESET);
            String helpPath = expand(BIN_DIR + "/hackerc-help");
            List<String> cmd = new ArrayList<>(List.of(helpPath));
            ProcessBuilder pb = new ProcessBuilder(cmd);
            pb.inheritIO();
            int code = pb.start().waitFor();
            if (code != 0) {
                System.out.println(RED + BOLD + "Help UI failed with code " + code + RESET);
                return 1;
            }
            System.out.println(GREEN + BOLD + "Help UI session completed." + RESET);
            return 0;
        }
    }

    @Command(name = "help", description = "Show this help menu")
    public static class Help implements Callable<Integer> {
        public Help() {}

        @Override public Integer call() {
            printCustomHelp(true);
            return 0;
        }
    }

    // ---------- Custom Help ----------
    private static void printCustomHelp(boolean showBanner) {
        if (showBanner) {
            System.out.println(PURPLE + BOLD + "Hacker Lang CLI - Advanced Scripting Tool" + RESET);
        }
        System.out.println(YELLOW + BOLD + "Commands Overview:" + RESET);
        String[][] commands = {
                {"run", "Execute a .hacker script", "file [--verbose] or . for bytes project"},
                {"compile", "Compile to native executable", "file [-o output] [--verbose] [--bytes]"},
                {"check", "Validate syntax", "file [--verbose]"},
                {"init", "Generate template script", "file [--verbose]"},
                {"clean", "Remove temporary files", "[--verbose]"},
                {"repl", "Launch interactive REPL", "[--verbose]"},
                {"editor", "Launch hacker-editor", "[file]"},
                {"unpack", "Unpack and install bytes", "bytes [--verbose]"},
                {"version", "Display version", ""},
                {"help", "Show this help menu", ""},
                {"help-ui", "Show special commands list", ""}
        };
        System.out.println(CYAN + String.format("%-15s %-40s %-40s", "Command", "Description", "Arguments") + RESET);
        for (String[] cmd : commands) {
            System.out.println(GREEN + String.format("%-15s", cmd[0]) + RESET + " " + WHITE + String.format("%-40s", cmd[1]) + RESET + " " + BLUE + cmd[2] + RESET);
        }
        System.out.println(YELLOW + BOLD + "\nSyntax Highlight Example:" + RESET);
        String exampleCode = "// sudo\n# obsidian\n@USER=admin\n=2 > echo $USER\n? [ -d /tmp ] > echo OK\n& sleep 10\n# logging\n> sudo apt update\n[\nConfig=Example\n]";
        System.out.println(CYAN + exampleCode + RESET);
        System.out.println(YELLOW + BOLD + "\nAdditional Information:" + RESET);
        System.out.println(CYAN + "- All commands support Debian-based systems." + RESET);
        System.out.println(CYAN + "- Use 'hackerc help-ui' for an interactive UI showing special syntax and commands." + RESET);
        System.out.println(CYAN + "- REPL supports multi-line input for complex scripts." + RESET);
        System.out.println(CYAN + "- Compilation produces native binaries for faster execution." + RESET);
        System.out.println(CYAN + "- Use 'unpack bytes' to install the bytes tool for project management." + RESET);
    }

    // ---------- Implementacje ----------
    private static void executeScript(String file, boolean verbose) throws Exception {
        System.out.println(CYAN + BOLD + "Parsing script..." + RESET);
        ParseResult r = runParser(file, verbose);
        if (!r.errors().isEmpty()) {
            System.out.println(RED + BOLD + "Syntax errors:" + RESET);
            r.errors().forEach(e -> System.out.println(RED + " - " + e + RESET));
            throw new RuntimeException("Syntax errors");
        }
        if (!r.libs().isEmpty()) {
            System.out.println(YELLOW + BOLD + "Warning: Missing custom libs: " + String.join(", ", r.libs()) + RESET);
        }
        System.out.println(BLUE + BOLD + "Config: " + r.config() + RESET);
        System.out.println(GREEN + BOLD + "Running..." + RESET);
        Path temp = Files.createTempFile("tmp", ".sh");
        try (BufferedWriter w = Files.newBufferedWriter(temp)) {
            w.write("#!/bin/bash\nset -e\n");
            r.vars().forEach((k,v) -> {
                try {
                    w.write("export " + k + "=\"" + v + "\"\n");
                } catch (IOException ex) {
                    throw new RuntimeException(ex);
                }
            });
            r.deps().stream().filter(d -> !"sudo".equals(d))
                    .forEach(d -> {
                        try {
                            w.write("command -v " + d + " || (sudo apt update && sudo apt install -y " + d + ")\n");
                        } catch (IOException ex) {
                            throw new RuntimeException(ex);
                        }
                    });
            r.includes().forEach(inc -> {
                try {
                    Path lib = Paths.get(expand(HACKER_DIR + "/libs/" + inc + "/main.hacker"));
                    w.write("# include " + inc + "\n");
                    List<String> libContent = Files.readAllLines(lib);
                    for (String l : libContent) {
                        w.write(l + "\n");
                    }
                    w.write("\n");
                } catch (Exception ex) {
                    System.out.println(RED + BOLD + "Cannot read lib " + inc + RESET);
                }
            });
            r.cmds().forEach(c -> {
                try {
                    w.write(c + "\n");
                } catch (IOException ex) {
                    throw new RuntimeException(ex);
                }
            });
            r.binaries().forEach(b -> {
                try {
                    w.write(b + "\n");
                } catch (IOException ex) {
                    throw new RuntimeException(ex);
                }
            });
            r.plugins().forEach(p -> {
                try {
                    w.write(p + " &\n");
                } catch (IOException ex) {
                    throw new RuntimeException(ex);
                }
            });
        }
        Files.setPosixFilePermissions(temp, Set.of(PosixFilePermission.OWNER_READ, PosixFilePermission.OWNER_WRITE, PosixFilePermission.OWNER_EXECUTE));
        System.out.println(CYAN + BOLD + "Executing " + file + RESET);
        ProcessBuilder pb = new ProcessBuilder("bash", temp.toString());
        pb.inheritIO();
        r.vars().forEach(pb.environment()::put);
        int code = pb.start().waitFor();
        Files.deleteIfExists(temp);
        if (code != 0) {
            throw new RuntimeException("Execution failed with code " + code);
        }
        System.out.println(GREEN + BOLD + "Execution completed successfully!" + RESET);
    }

    private static void runBytesProject(boolean verbose) throws Exception {
        Yaml yaml = new Yaml();
        Map<String, Object> proj = yaml.load(Files.newInputStream(Paths.get("hacker.bytes")));
        @SuppressWarnings("unchecked")
        Map<String, Object> pkg = (Map<String, Object>) proj.get("package");
        String entry = (String) proj.get("entry");
        String name = (String) pkg.get("name");
        String version = (String) pkg.get("version");
        String author = (String) pkg.get("author");
        System.out.println(GREEN + BOLD + "Running project " + name + " v" + version + " by " + author + RESET);
        executeScript(entry, verbose);
    }

    private static void compileNormal(String file, String output, boolean verbose) throws Exception {
        if (output == null || output.isEmpty()) output = file.replaceAll("\\.hacker$", "");
        String compiler = expand(BIN_DIR + "/hacker-compiler");
        List<String> cmd = new ArrayList<>(List.of(compiler, file, output));
        if (verbose) cmd.add("--verbose");
        System.out.println(CYAN + BOLD + "Compiling " + file + " to " + output + RESET);
        ProcessBuilder pb = new ProcessBuilder(cmd);
        pb.inheritIO();
        int code = pb.start().waitFor();
        if (code != 0) {
            throw new RuntimeException("Compilation failed");
        }
        System.out.println(GREEN + BOLD + "Compilation successful!" + RESET);
    }

    private static void compileBytesProject(String output, boolean verbose) throws Exception {
        Yaml yaml = new Yaml();
        Map<String, Object> proj = yaml.load(Files.newInputStream(Paths.get("hacker.bytes")));
        @SuppressWarnings("unchecked")
        Map<String, Object> pkg = (Map<String, Object>) proj.get("package");
        String entry = (String) proj.get("entry");
        if (output.isEmpty()) output = (String) pkg.get("name");
        String name = (String) pkg.get("name");
        System.out.println(CYAN + BOLD + "Compiling project " + name + " to " + output + " with --bytes" + RESET);
        String compiler = expand(BIN_DIR + "/hacker-compiler");
        List<String> cmd = new ArrayList<>(List.of(compiler, entry, output, "--bytes"));
        if (verbose) cmd.add("--verbose");
        ProcessBuilder pb = new ProcessBuilder(cmd);
        pb.inheritIO();
        int code = pb.start().waitFor();
        if (code != 0) {
            throw new RuntimeException("Compilation failed");
        }
        System.out.println(GREEN + BOLD + "Compilation successful!" + RESET);
    }

    private static void startRepl(boolean verbose) throws Exception {
        Terminal terminal = TerminalBuilder.builder().system(true).build();
        DefaultHistory history = new DefaultHistory();
        Path histFile = Paths.get(expand(HISTORY_FILE));
        if (Files.exists(histFile)) {
            history.read(histFile, false);
        }
        LineReader reader = LineReaderBuilder.builder()
                .terminal(terminal)
                .history(history)
                .build();
        System.out.println(PURPLE + BOLD + "Hacker Lang REPL v" + VERSION + " - Enhanced Interactive Mode" + RESET);
        System.out.println(CYAN + "Type 'exit' to quit, 'help' for commands, 'clear' to reset" + RESET);
        System.out.println(CYAN + "Supported: //deps, #libs, @vars, =loops, ?ifs, &bg, >cmds, [config], !comments" + RESET);
        List<String> lines = new ArrayList<>();
        boolean inConfig = false;
        while (true) {
            String prompt = inConfig ? BLUE + BOLD + "CONFIG> " + RESET : GREEN + BOLD + "hacker> " + RESET;
            String line;
            try {
                line = reader.readLine(prompt);
            } catch (UserInterruptException | EndOfFileException e) {
                break;
            }
            if (line == null || line.trim().isEmpty()) continue;
            if (line.equals("exit")) break;
            if (line.equals("help")) {
                System.out.println(CYAN + "REPL Commands:" + RESET);
                System.out.println(CYAN + "- exit: Quit REPL" + RESET);
                System.out.println(CYAN + "- help: This menu" + RESET);
                System.out.println(CYAN + "- clear: Reset session" + RESET);
                System.out.println(CYAN + "- verbose: Toggle verbose" + RESET);
                continue;
            }
            if (line.equals("clear")) {
                lines.clear();
                System.out.print("\033[H\033[2J");
                System.out.println(GREEN + "Session cleared!" + RESET);
                continue;
            }
            if (line.equals("verbose")) {
                verbose = !verbose;
                System.out.println(YELLOW + "Verbose mode: " + verbose + RESET);
                continue;
            }
            if (line.equals("[")) inConfig = true;
            if (line.equals("]")) inConfig = false;
            lines.add(line);
            if (!inConfig && !line.isBlank() && !line.startsWith("!")) {
                Path temp = Files.createTempFile("repl_", ".hacker");
                Files.writeString(temp, String.join("\n", lines));
                try {
                    ParseResult r = runParser(temp.toString(), verbose);
                    if (!r.errors().isEmpty()) {
                        System.out.println(RED + BOLD + "REPL Errors:" + RESET);
                        r.errors().forEach(e -> System.out.println(RED + " - " + e + RESET));
                    } else {
                        executeScript(temp.toString(), verbose);
                    }
                } catch (Exception ex) {
                    System.out.println(RED + BOLD + ex.getMessage() + RESET);
                } finally {
                    Files.deleteIfExists(temp);
                }
            }
        }
        history.write(histFile, false);
        System.out.println(GREEN + BOLD + "REPL session ended." + RESET);
    }

    // ---------- main + welcome ----------
    public static void main(String[] args) throws Exception {
        ensureDirs();
        CommandLine cli = new CommandLine(new HackerCLI());
        if (args.length == 0) {
            System.out.println(PURPLE + BOLD + "Welcome to Hacker Lang CLI v" + VERSION + RESET);
            System.out.println(CYAN + "Advanced scripting for Debian-based Linux systems" + RESET);
            System.out.println(BLUE + "Type 'hackerc help' for commands or 'hackerc repl' to start interactive mode." + RESET);
            printCustomHelp(false);
            return;
        }
        int exitCode = cli.execute(args);
        System.exit(exitCode);
    }

    @Override
    public Integer call() throws Exception {
        printCustomHelp(true);
        return 0;
    }
}
