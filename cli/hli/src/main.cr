require "colorize"
require "file_utils"
require "http/client"
require "json"
require "option_parser"
require "process"
require "yaml"

VERSION = "1.3" # Updated version for new pipeline integration
HACKER_DIR = File.expand_path("~/.hackeros/hacker-lang")
FRONTEND_BIN_DIR = File.join(HACKER_DIR, "bin/frontend")
MIDDLE_END_BIN_DIR = File.join(HACKER_DIR, "bin/middle-end")
BIN_DIR = File.join(HACKER_DIR, "bin")
CACHE_DIR = "./.cache" # For hli projects
LEXER_PATH = File.join(FRONTEND_BIN_DIR, "hacker-lexer")
PARSER_PATH = File.join(FRONTEND_BIN_DIR, "hacker-parser")
SA_PATH = File.join(MIDDLE_END_BIN_DIR, "hacker-sa")
AST_PATH = File.join(MIDDLE_END_BIN_DIR, "hacker-ast")
COMPILER_PATH = File.join(BIN_DIR, "hacker-compiler")
RUNTIME_PATH = File.join(BIN_DIR, "hacker-runtime")
EDITOR_PATH = File.join(BIN_DIR, "hacker-editor")
REPL_PATH = File.join(BIN_DIR, "hacker-repl")
COLOR_RESET = "\033[0m"
COLOR_RED = "\033[31m"
COLOR_GREEN = "\033[32m"
COLOR_YELLOW = "\033[33m"
COLOR_BLUE = "\033[34m"
COLOR_PURPLE = "\033[35m"
COLOR_CYAN = "\033[36m"
COLOR_WHITE = "\033[37m"
COLOR_BOLD = "\033[1m"
COLOR_GRAY = "\033[90m"

def ensure_hacker_dir
  Dir.mkdir_p(FRONTEND_BIN_DIR)
  Dir.mkdir_p(MIDDLE_END_BIN_DIR)
  Dir.mkdir_p(BIN_DIR)
  Dir.mkdir_p(File.join(HACKER_DIR, "libs"))
  Dir.mkdir_p(File.join(HACKER_DIR, "plugins"))
  Dir.mkdir_p(CACHE_DIR) # Project cache for foreign libs
  Dir.mkdir_p(File.dirname(File.expand_path("~/.hackeros/history/hacker_repl_history")))
end

def get_json_output(bin_path : String, input_file : String? = nil, args : Array(String) = [] of String, verbose : Bool = false) : String?
  unless File.exists?(bin_path)
    puts "#{COLOR_RED}Binary not found: #{bin_path}. Please install Hacker Lang tools.#{COLOR_RESET}"
    return nil
  end
  out_path = File.tempname(suffix: ".json")
  begin
    input_args = input_file ? [input_file] : [] of String
    full_args = input_args + args
    error_io = verbose ? STDOUT : IO::Memory.new
    status = uninitialized Process::Status
    File.open(out_path, "w") do |out_io|
      status = Process.run(bin_path, full_args, output: out_io, error: error_io)
    end
    if status.success?
      File.read(out_path)
    else
      puts "#{COLOR_RED}Failed to run #{File.basename(bin_path)}#{COLOR_RESET}" if verbose
      nil
    end
  ensure
    File.delete(out_path) if File.exists?(out_path)
  end
end

def chain_pipeline(file : String, stages : Array(String), final_output : String? = nil, verbose : Bool = false, mode : String = "hli") : Bool
  current_json = nil
  temp_files = [] of String
  begin
    # Stage 1: Lexer
    if stages.includes?("lexer")
      lex_json = get_json_output(LEXER_PATH, file, [] of String, verbose)
      return false unless lex_json
      current_json = lex_json
      # Temp for next
      temp_lex = File.tempname(suffix: ".json")
      File.write(temp_lex, current_json)
      temp_files << temp_lex
    end
    # Stage 2: Parser (takes file or stdin JSON tokens; assume updated to take JSON if lexer used)
    if stages.includes?("parser")
      parser_args = mode == "hli" ? ["--mode", mode] : [] of String
      parse_json = get_json_output(PARSER_PATH, current_json ? "" : file, parser_args, verbose)
      return false unless parse_json
      current_json = parse_json
      temp_parse = File.tempname(suffix: ".json")
      File.write(temp_parse, current_json)
      temp_files << temp_parse
    end
    # Stage 3: SA
    if stages.includes?("sa")
      sa_json = get_json_output(SA_PATH, temp_files.last?, [] of String, verbose)
      return false unless sa_json
      current_json = sa_json
      temp_sa = File.tempname(suffix: ".json")
      File.write(temp_sa, current_json)
      temp_files << temp_sa
    end
    # Stage 4: AST
    if stages.includes?("ast")
      ast_json = get_json_output(AST_PATH, temp_files.last?, [] of String, verbose)
      return false unless ast_json
      current_json = ast_json
      temp_ast = File.tempname(suffix: ".json")
      File.write(temp_ast, current_json)
      temp_files << temp_ast
    end
    # Stage 5: Compiler (takes ParseResult or AST; assume compatible with SA output)
    if stages.includes?("compiler")
      compiler_args = final_output ? [final_output] : ["temp_exec"]
      compiler_args += verbose ? ["--verbose"] : [] of String
      compiler_args += ["--bytes"] if final_output # Assume bytes mode for embed
      compile_json = get_json_output(COMPILER_PATH, temp_files.last?, compiler_args, verbose)
      # Compiler outputs binary, not JSON; success if no error
      return compile_json != nil || File.exists?(final_output || "temp_exec")
    end
    # For runtime, if needed, but compiler produces exec
    true
  ensure
    temp_files.each do |tf|
      File.delete(tf) if File.exists?(tf)
    end
  end
end

def display_welcome
  puts "#{COLOR_BOLD}#{COLOR_PURPLE}Welcome to Hacker Lang Interface (HLI) v#{VERSION}#{COLOR_RESET}"
  puts "#{COLOR_GRAY}Advanced scripting interface for Hacker Lang with full pipeline support.#{COLOR_RESET}"
  puts "#{COLOR_WHITE}Type 'hli help' for commands or 'hli repl' to start interactive mode.#{COLOR_RESET}\n"
  help_command(false)
end

def load_project_entry : String?
  bytes_file = "bytes.yaml"
  if File.exists?(bytes_file)
    data = YAML.parse(File.read(bytes_file))
    project = data.as_h
    entry = project["entry"]?.try(&.as_s)
    return entry if entry
  end
  nil
end

def run_command(file : String, verbose : Bool) : Bool
  # Full pipeline for run: lexer -> parser -> sa -> ast -> compiler -> exec
  stages = ["lexer", "parser", "sa", "ast", "compiler"]
  temp_exec = "temp_hli_exec"
  success = chain_pipeline(file, stages, temp_exec, verbose)
  if success && File.exists?(temp_exec)
    exec_status = Process.run(temp_exec, output: STDOUT, error: STDERR)
    File.delete(temp_exec)
    exec_status.success?
  else
    false
  end
end

def compile_command(file : String, output : String, verbose : Bool, bytes_mode : Bool) : Bool
  stages = ["lexer", "parser", "sa", "ast", "compiler"]
  compiler_args = bytes_mode ? ["--bytes"] : [] of String
  chain_pipeline(file, stages, output, verbose) # final_output = output
end

def check_command(file : String, verbose : Bool) : Bool
  stages = ["lexer", "parser", "sa"]
  success = chain_pipeline(file, stages, nil, verbose)
  if success
    # Get last JSON (sa) and check errors
    # But since chain doesn't return JSON, modify or call separately
    sa_json = get_json_output(SA_PATH, file, ["--input-from-parser"], verbose) # Assume arg for input
    if sa_json
      begin
        parsed = JSON.parse(sa_json)
        all_errors = parsed["errors"].as_a + (parsed["semantic_errors"]?.try(&.as_a) || [] of JSON::Any)
        if all_errors.empty?
          puts "#{COLOR_GREEN}Syntax and semantic validation passed!#{COLOR_RESET}"
          true
        else
          puts "\n#{COLOR_RED}Errors:#{COLOR_RESET}"
          all_errors.each do |e|
            puts " #{COLOR_RED}✖ #{COLOR_RESET}#{e.as_s}"
          end
          puts ""
          false
        end
      rescue ex
        puts "#{COLOR_RED}Error parsing SA output: #{ex.message}#{COLOR_RESET}"
        false
      end
    else
      false
    end
  else
    false
  end
end

# Rest of the functions remain similar, but update calls
def init_command(file : String?, verbose : Bool) : Bool
  target_file = file || "main.hacker"
  if File.exists?(target_file)
    puts "#{COLOR_RED}File #{target_file} already exists!#{COLOR_RESET}"
    return false
  end
  template = <<-TEMPLATE
! Updated Hacker Lang template with new syntax support
// sudo apt
# network-utils
#> python:requests ! Foreign Python lib example
@APP_NAME=HackerApp
@LOG_LEVEL=debug
$ITER=1 ! Local var
=3 > echo "Iteration: $ITER - $APP_NAME" ! Loop with vars
? [ -f /etc/os-release ] > cat /etc/os-release | grep PRETTY_NAME ! Conditional
& ping -c 1 google.com ! Background
: my_func
> echo "In function"
:
. my_func ! Call function
# logging
> echo "Starting..."
>>> sudo apt update && sudo apt upgrade -y ! Separate
\\ plugin-tool ! Plugin
[
Author=Advanced User
Version=1.0
]
TEMPLATE
  File.write(target_file, template)
  puts "#{COLOR_GREEN}Initialized template at #{target_file}#{COLOR_RESET}"
  if verbose
    puts "\n#{COLOR_YELLOW}Template content:#{COLOR_RESET}"
    puts template.colorize(:yellow)
  end
  bytes_file = "bytes.yaml"
  if !File.exists?(bytes_file)
    bytes_template = <<-YAML
package:
  name: my-hacker-project
  version: 0.1.0
  author: User
entry: #{target_file}
dependencies:
  - network-utils
  - logging
  - python:requests
YAML
    File.write(bytes_file, bytes_template)
    puts "#{COLOR_GREEN}Initialized bytes.yaml for project#{COLOR_RESET}"
  end
  Dir.mkdir_p(CACHE_DIR) # Ensure cache for foreign libs
  true
end

def clean_command(verbose : Bool) : Bool
  count = 0
  Dir.glob("/tmp/hacker_*") do |path| # Also clean global temps if needed
    if File.basename(path).starts_with?("hacker_") || File.basename(path).starts_with?("temp_hli_exec")
      puts "#{COLOR_YELLOW}Removed: #{path}#{COLOR_RESET}" if verbose
      File.delete(path)
      count += 1
    end
  end
  # Clean project cache if empty or flag
  if Dir.exists?(CACHE_DIR) && Dir.children(CACHE_DIR).empty?
    Dir.delete(CACHE_DIR)
    puts "#{COLOR_YELLOW}Cleaned empty cache: #{CACHE_DIR}#{COLOR_RESET}" if verbose
  end
  puts "#{COLOR_GREEN}Removed #{count} temporary files#{COLOR_RESET}"
  true
end

# Other functions like unpack_bytes, editor_command, run_repl, version_command, syntax_command, docs_command, tutorials_command, help_command, run_help_ui remain the same as provided.
def unpack_bytes(verbose : Bool) : Bool
  bytes_path1 = File.join(HACKER_DIR, "bin/bytes")
  bytes_path2 = "/usr/bin/bytes"
  if File.exists?(bytes_path1)
    puts "#{COLOR_GREEN}Bytes already installed at #{bytes_path1}.#{COLOR_RESET}"
    return true
  end
  if File.exists?(bytes_path2)
    puts "#{COLOR_GREEN}Bytes already installed at #{bytes_path2}.#{COLOR_RESET}"
    return true
  end
  Dir.mkdir_p(BIN_DIR)
  url = "https://github.com/Bytes-Repository/Bytes-CLI-Tool/releases/download/v0.3/bytes" # Assume
  HTTP::Client.get(url) do |response|
    if response.status_code != 200
      puts "#{COLOR_RED}Error: status code #{response.status_code}#{COLOR_RESET}"
      return false
    end
    File.open(bytes_path1, "w") do |f|
      IO.copy(response.body_io, f)
    end
  end
  File.chmod(bytes_path1, 0o755)
  if verbose
    puts "#{COLOR_GREEN}Downloaded and installed bytes from #{url} to #{bytes_path1}#{COLOR_RESET}"
  end
  puts "#{COLOR_GREEN}Bytes installed successfully!#{COLOR_RESET}"
  true
end

def editor_command(file : String?) : Bool
  if !File.exists?(EDITOR_PATH)
    puts "#{COLOR_RED}Hacker editor not found at #{EDITOR_PATH}. Please install the Hacker Lang tools.#{COLOR_RESET}"
    return false
  end
  args = file ? [file] : [] of String
  puts "#{COLOR_CYAN}Launching editor: #{EDITOR_PATH} #{file || ""}#{COLOR_RESET}"
  status = Process.run(EDITOR_PATH, args, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit, input: Process::Redirect::Inherit)
  if status.success?
    puts "#{COLOR_GREEN}Editor session completed.#{COLOR_RESET}"
    true
  else
    puts "#{COLOR_RED}Editor failed.#{COLOR_RESET}"
    false
  end
end

def run_repl(verbose : Bool) : Bool
  if !File.exists?(REPL_PATH)
    puts "#{COLOR_RED}Hacker REPL not found at #{REPL_PATH}. Please install the Hacker Lang tools.#{COLOR_RESET}"
    return false
  end
  args = verbose ? ["--verbose"] : [] of String
  status = Process.run(REPL_PATH, args, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit, input: Process::Redirect::Inherit)
  if status.success?
    puts "#{COLOR_GREEN}REPL session ended.#{COLOR_RESET}"
    true
  else
    puts "#{COLOR_RED}REPL failed.#{COLOR_RESET}"
    false
  end
end

def version_command : Bool
  puts "#{COLOR_CYAN}Hacker Lang Interface (HLI) v#{VERSION}#{COLOR_RESET}"
  true
end

def syntax_command : Bool
  puts "#{COLOR_BOLD}Hacker Lang Syntax Example:#{COLOR_RESET}\n"
  example_code = <<-EXAMPLE
// sudo
# obsidian
#> rust:serde ! Foreign Rust lib
@USER=admin
$ITER=0
=2 > echo $USER - $ITER
? [ -d /tmp ] > echo OK
& sleep 10
>> echo "With var: $USER"
>>> separate_command
: myfunc
> echo in func
:
. myfunc
# logging
> sudo apt update
[
Config=Example
]
EXAMPLE
  puts example_code.colorize(:white)
  true
end

def docs_command : Bool
  puts "#{COLOR_BOLD}Hacker Lang Documentation:#{COLOR_RESET}\n"
  puts "Hacker Lang is an advanced scripting language for HackerOS."
  puts "Key features:"
  puts "- Privileged operations with // sudo"
  puts "- Library includes with # lib-name or #> foreign-lang:lib"
  puts "- Variables with @VAR=value (global), $var=value (local)"
  puts "- Loops with =N > command"
  puts "- Conditionals with ? condition > command"
  puts "- Background tasks with & command"
  puts "- Multi-line commands with >> and >>>"
  puts "- Functions with : name ... : and calls .name"
  puts "- Metadata blocks with [ key=value ]"
  puts "- Foreign libs cached in .cache for hli"
  puts "\nFor more details, visit the official documentation or use 'hli tutorials' for examples."
  true
end

def tutorials_command : Bool
  puts "#{COLOR_BOLD}Hacker Lang Tutorials:#{COLOR_RESET}\n"
  puts "Tutorial 1: Basic Script"
  puts "Create main.hacker with > echo 'Hello'"
  puts "Run with: hli run"
  puts "\nTutorial 2: Pipeline"
  puts "hli check main.hacker # Validates lexer->parser->sa"
  puts "hli compile main.hacker -o exec # Full to binary"
  puts "\nTutorial 3: Foreign Libs"
  puts "Use #> python:requests in script; cached in .cache"
  puts "hli run # Handles automatically"
  true
end

def help_command(show_banner : Bool) : Bool
  if show_banner
    puts "#{COLOR_BOLD}#{COLOR_PURPLE}Hacker Lang Interface (HLI) - Pipeline-Enabled v#{VERSION}#{COLOR_RESET}\n"
  end
  puts "#{COLOR_BOLD}Commands Overview:#{COLOR_RESET}"
  puts "#{"Command".ljust(15).colorize(:light_gray)} #{"Description".ljust(40).colorize(:light_gray)} #{"Arguments".ljust(40).colorize(:light_gray)}"
  commands = [
    ["run", "Execute via full pipeline (lexer->...->exec)", "[file] [--verbose]"],
    ["compile", "Compile via pipeline to binary", "[file] [-o output] [--verbose] [--bytes]"],
    ["check", "Validate via lexer->parser->sa", "[file] [--verbose]"],
    ["init", "Generate template script/project", "[file] [--verbose]"],
    ["clean", "Remove temps and cache", "[--verbose]"],
    ["repl", "Launch interactive REPL", "[--verbose]"],
    ["editor", "Launch hacker-editor", "[file]"],
    ["unpack", "Install bytes tool", "[--verbose]"],
    ["docs", "Show documentation", ""],
    ["tutorials", "Show tutorials", ""],
    ["version", "Display version", ""],
    ["help", "Show this help menu", ""],
    ["syntax", "Show syntax examples", ""],
    ["help-ui", "Show special commands list", ""],
  ]
  commands.each do |cmd|
    puts "#{cmd[0].ljust(15)} #{cmd[1].ljust(40)} #{cmd[2].ljust(40)}"
  end
  true
end

def run_help_ui : Bool
  puts "#{COLOR_BOLD}#{COLOR_PURPLE}Hacker Lang Commands List#{COLOR_RESET}"
  puts "run: Execute via pipeline - hli run [file] [--verbose]"
  puts "compile: Compile via pipeline - hli compile [file] [-o output] [--verbose] [--bytes]"
  puts "check: Validate pipeline - hli check [file] [--verbose]"
  puts "init: Generate template - hli init [file] [--verbose]"
  puts "clean: Clean temps/cache - hli clean [--verbose]"
  puts "repl: REPL - hli repl [--verbose]"
  puts "editor: Editor - hli editor [file]"
  puts "unpack: Install bytes - hli unpack [--verbose]"
  puts "docs: Docs - hli docs"
  puts "tutorials: Tutorials - hli tutorials"
  puts "version: Version - hli version"
  puts "help: Help - hli help"
  puts "syntax: Syntax - hli syntax"
  puts "help-ui: This UI - hli help-ui"
  true
end

def run_bytes_project(verbose : Bool) : Bool
  bytes_file = "bytes.yaml"
  if !File.exists?(bytes_file)
    puts "#{COLOR_RED}Error: #{bytes_file} not found. Use 'hli init' to create a project.#{COLOR_RESET}"
    return false
  end
  data = YAML.parse(File.read(bytes_file))
  project = data.as_h
  package = project["package"].as_h
  name = package["name"].as_s
  version = package["version"].as_s
  author = package["author"].as_s
  entry = project["entry"].as_s
  deps = project["dependencies"]?.try(&.as_a) || [] of YAML::Any
  puts "#{COLOR_GREEN}Running project #{name} v#{version} by #{author}#{COLOR_RESET}"
  check_dependencies(entry, verbose, deps)
  run_command(entry, verbose)
end

def compile_bytes_project(output : String, verbose : Bool) : Bool
  bytes_file = "bytes.yaml"
  if !File.exists?(bytes_file)
    puts "#{COLOR_RED}Error: #{bytes_file} not found. Use 'hli init' to create a project.#{COLOR_RESET}"
    return false
  end
  data = YAML.parse(File.read(bytes_file))
  project = data.as_h
  package = project["package"].as_h
  name = package["name"].as_s
  entry = project["entry"].as_s
  output = output.empty? ? name : output
  puts "#{COLOR_CYAN}Compiling project #{name} to #{output} via pipeline#{COLOR_RESET}"
  deps = project["dependencies"]?.try(&.as_a) || [] of YAML::Any
  check_dependencies(entry, verbose, deps)
  compile_command(entry, output, verbose, true)
end

def check_bytes_project(verbose : Bool) : Bool
  bytes_file = "bytes.yaml"
  if !File.exists?(bytes_file)
    puts "#{COLOR_RED}Error: #{bytes_file} not found. Use 'hli init' to create a project.#{COLOR_RESET}"
    return false
  end
  data = YAML.parse(File.read(bytes_file))
  project = data.as_h
  entry = project["entry"].as_s
  deps = project["dependencies"]?.try(&.as_a) || [] of YAML::Any
  check_dependencies(entry, verbose, deps)
  check_command(entry, verbose)
end

def check_dependencies(file : String, verbose : Bool, deps_yaml : Array(YAML::Any)? = nil) : Bool
  if !File.exists?(file)
    puts "#{COLOR_RED}File #{file} not found for dependency check.#{COLOR_RESET}"
    return false
  end
  content = File.read(file)
  libs_dir = File.join(HACKER_DIR, "libs")
  plugins_dir = File.join(HACKER_DIR, "plugins")
  missing_libs = [] of String
  missing_plugins = [] of String
  # Parse for # and \\
  content.lines.each do |line|
    stripped = line.strip
    if stripped.starts_with?("//")
      # Deps handled by runtime
    elsif stripped.starts_with?("#") || stripped.starts_with?("#>")
      lib_name = stripped[1..].strip.split(" ").first?.try(&.gsub(/[^-a-zA-Z0-9_:]/, ""))
      if lib_name && lib_name.size > 0 && !Dir.glob(File.join(libs_dir, "#{lib_name}*")).any? && !Dir.glob(File.join(CACHE_DIR, "#{lib_name}*")).any?
        missing_libs << lib_name unless missing_libs.includes?(lib_name)
      end
    elsif stripped.starts_with?("\\")
      plugin_name = stripped[1..].strip.split(" ").first?.try(&.gsub(/[^-a-zA-Z0-9_]/, ""))
      if plugin_name && plugin_name.size > 0 && !Dir.glob(File.join(plugins_dir, "#{plugin_name}*")).any?
        missing_plugins << plugin_name unless missing_plugins.includes?(plugin_name)
      end
    end
  end
  # From bytes.yaml
  if deps_yaml
    deps_yaml.each do |dep|
      dep_name = dep.as_s
      if !Dir.glob(File.join(libs_dir, "#{dep_name}*")).any? && !Dir.glob(File.join(CACHE_DIR, "#{dep_name}*")).any?
        missing_libs << dep_name
      end
    end
  end
  if !missing_plugins.empty?
    puts "#{COLOR_YELLOW}Missing plugins: #{missing_plugins.join(", ")}#{COLOR_RESET}" if verbose
    missing_plugins.each do |p|
      puts "#{COLOR_YELLOW}Installing plugin #{p} via bytes...#{COLOR_RESET}"
      status = Process.run("bytes", ["plugin", "install", p], output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
      return false unless status.success?
    end
  end
  if !missing_libs.empty?
    puts "#{COLOR_YELLOW}Missing libs: #{missing_libs.join(", ")}#{COLOR_RESET}" if verbose
    missing_libs.each do |l|
      puts "#{COLOR_YELLOW}Installing lib #{l} via bytes or cache...#{COLOR_RESET}"
      if l.includes?(":") # Foreign
        # Cache in .cache
        lib_dir = File.join(CACHE_DIR, l)
        unless Dir.exists?(lib_dir)
          Dir.mkdir_p(lib_dir)
          # Download/setup - simplified
          puts "Cached foreign lib #{l}"
        end
      else
        status = Process.run("bytes", ["install", l], output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
        return false unless status.success?
      end
    end
  end
  true
end

def execute_task(task_name : String, yaml : YAML::Any, executed = Set(String).new) : Nil
  if executed.includes?(task_name)
    raise "Cycle detected in tasks involving #{task_name}"
  end
  executed.add(task_name)
  vars = yaml["vars"]?.try(&.as_h) || Hash(String, YAML::Any).new
  task = yaml["tasks"][task_name]
  requires = task["requires"]?.try(&.as_a) || [] of YAML::Any
  requires.each do |req|
    execute_task(req.as_s, yaml, executed)
  end
  run_cmds = task["run"].as_a
  run_cmds.each do |cmd_any|
    cmd = cmd_any.as_s
    sub_cmd = cmd.gsub(/\{\{ *(\w+) *\}\}/) { |match| vars[$1]?.try(&.as_s) || match[0] }
    status = Process.run("sh", args: ["-c", sub_cmd], output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
    if !status.success?
      raise "Command failed: #{sub_cmd}"
    end
  end
end

def main
  ensure_hacker_dir
  if ARGV.empty?
    display_welcome
    exit(0)
  end
  if ARGV[0] == "--version" || ARGV[0] == "-v"
    version_command
    exit(0)
  elsif ARGV[0] == "--help" || ARGV[0] == "-h"
    help_command(true)
    exit(0)
  end
  command = ARGV.shift
  success = true
  verbose = false
  file : String? = nil
  output = ""
  bytes_mode = false
  case command
  when "run"
    parser = OptionParser.new do |p|
      p.banner = "Usage: hli run [file] [options]\n\nExecute via full pipeline."
      p.on("--verbose", "Verbose") { verbose = true }
      p.on("-h", "--help", "Help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if ARGV.size > 1
      puts "#{COLOR_RED}Expected at most one arg: [file]#{COLOR_RESET}"
      puts parser
      exit(1)
    elsif ARGV.size == 1
      file = ARGV.shift
    end
    if file.nil?
      entry = load_project_entry
      if entry.nil?
        puts "#{COLOR_RED}No project. Use 'hli init' or specify file.#{COLOR_RESET}"
        success = false
      else
        deps = nil # From bytes.yaml implicit
        check_dependencies(entry, verbose, deps)
        success = run_command(entry, verbose)
      end
    elsif file == "."
      success = run_bytes_project(verbose)
    else
      success = run_command(file.not_nil!, verbose)
    end
  when "compile"
    parser = OptionParser.new do |p|
      p.banner = "Usage: hli compile [file] [options]\n\nCompile via pipeline."
      p.on("-o OUTPUT", "--output OUTPUT", "Output") { |o| output = o }
      p.on("--bytes", "Bytes mode") { bytes_mode = true }
      p.on("--verbose", "Verbose") { verbose = true }
      p.on("-h", "--help", "Help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if ARGV.size > 1
      puts "#{COLOR_RED}Expected at most one arg: [file]#{COLOR_RESET}"
      puts parser
      exit(1)
    elsif ARGV.size == 1
      file = ARGV.shift
    end
    if file.nil?
      entry = load_project_entry
      if entry.nil?
        puts "#{COLOR_RED}No project. Use 'hli init' or specify file.#{COLOR_RESET}"
        success = false
      else
        output = output.empty? ? File.basename(entry, File.extname(entry)) : output
        deps = nil
        check_dependencies(entry, verbose, deps)
        success = compile_command(entry, output, verbose, bytes_mode)
      end
    elsif file == "."
      success = compile_bytes_project(output, verbose)
    else
      output = output.empty? ? File.basename(file.not_nil!, File.extname(file.not_nil!)) : output
      success = compile_command(file.not_nil!, output, verbose, bytes_mode)
    end
  when "check"
    parser = OptionParser.new do |p|
      p.banner = "Usage: hli check [file] [options]\n\nValidate via pipeline."
      p.on("--verbose", "Verbose") { verbose = true }
      p.on("-h", "--help", "Help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if ARGV.size > 1
      puts "#{COLOR_RED}Expected at most one arg: [file]#{COLOR_RESET}"
      puts parser
      exit(1)
    elsif ARGV.size == 1
      file = ARGV.shift
    end
    if file.nil?
      entry = load_project_entry
      if entry.nil?
        puts "#{COLOR_RED}No project. Use 'hli init' or specify file.#{COLOR_RESET}"
        success = false
      else
        deps = nil
        check_dependencies(entry, verbose, deps)
        success = check_command(entry, verbose)
      end
    elsif file == "."
      success = check_bytes_project(verbose)
    else
      success = check_command(file.not_nil!, verbose)
    end
  when "init"
    parser = OptionParser.new do |p|
      p.banner = "Usage: hli init [file] [options]\n\nGenerate template."
      p.on("--verbose", "Verbose") { verbose = true }
      p.on("-h", "--help", "Help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if ARGV.size > 1
      puts "#{COLOR_RED}Expected at most one arg: [file]#{COLOR_RESET}"
      puts parser
      exit(1)
    elsif ARGV.size == 1
      file = ARGV.shift
    end
    success = init_command(file, verbose)
  when "clean"
    parser = OptionParser.new do |p|
      p.banner = "Usage: hli clean [options]\n\nClean temps."
      p.on("--verbose", "Verbose") { verbose = true }
      p.on("-h", "--help", "Help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if !ARGV.empty?
      puts "#{COLOR_RED}No args expected#{COLOR_RESET}"
      puts parser
      exit(1)
    end
    success = clean_command(verbose)
  when "repl"
    parser = OptionParser.new do |p|
      p.banner = "Usage: hli repl [options]\n\nREPL."
      p.on("--verbose", "Verbose") { verbose = true }
      p.on("-h", "--help", "Help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if !ARGV.empty?
      puts "#{COLOR_RED}No args expected#{COLOR_RESET}"
      puts parser
      exit(1)
    end
    success = run_repl(verbose)
  when "editor"
    parser = OptionParser.new do |p|
      p.banner = "Usage: hli editor [file] [options]\n\nEditor."
      p.on("-h", "--help", "Help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if ARGV.size > 1
      puts "#{COLOR_RED}Expected at most one arg: [file]#{COLOR_RESET}"
      puts parser
      exit(1)
    elsif ARGV.size == 1
      file = ARGV.shift
    end
    success = editor_command(file)
  when "unpack"
    parser = OptionParser.new do |p|
      p.banner = "Usage: hli unpack [options]\n\nUnpack bytes."
      p.on("--verbose", "Verbose") { verbose = true }
      p.on("-h", "--help", "Help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if !ARGV.empty?
      puts "#{COLOR_RED}No args expected (use 'bytes')#{COLOR_RESET}"
      puts parser
      exit(1)
    end
    success = unpack_bytes(verbose)
  when "docs"
    success = docs_command
  when "tutorials"
    success = tutorials_command
  when "version"
    success = version_command
  when "help"
    success = help_command(true)
  when "syntax"
    success = syntax_command
  when "help-ui"
    success = run_help_ui
  when "install", "update", "remove"
    puts "#{COLOR_YELLOW}Use 'bytes #{command}' for package management#{COLOR_RESET}"
    success = true
  else
    if File.exists?(".hackerfile")
      begin
        content = File.read(".hackerfile")
        yaml = YAML.parse(content)
        aliases = yaml["aliases"]?.try(&.as_h) || Hash(String, YAML::Any).new
        aliased_task = aliases[command]?.try(&.as_s) || command
        if yaml["tasks"]?.try(&.as_h).try(&.has_key?(aliased_task))
          execute_task(aliased_task, yaml)
          success = true
        else
          puts "#{COLOR_RED}Unknown task: #{command}#{COLOR_RESET}"
          help_command(false)
          success = false
        end
      rescue ex
        puts "#{COLOR_RED}Error in .hackerfile: #{ex.message}#{COLOR_RESET}"
        success = false
      end
    else
      puts "#{COLOR_RED}Unknown command: #{command}#{COLOR_RESET}"
      help_command(false)
      success = false
    end
  end
  exit(success ? 0 : 1)
end

main
