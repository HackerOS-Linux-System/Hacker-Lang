require "colorize"
require "file_utils"
require "http/client"
require "json"
require "option_parser"
require "process"
require "yaml"

VERSION = "1.2"
HACKER_DIR = File.expand_path("~/.hackeros/hacker-lang")
BIN_DIR = File.join(HACKER_DIR, "bin")
HISTORY_FILE = File.expand_path("~/.hackeros/history/hacker_repl_history")
PARSER_PATH = File.join(BIN_DIR, "hacker-parser")
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
  Dir.mkdir_p(BIN_DIR)
  Dir.mkdir_p(File.join(HACKER_DIR, "libs"))
  Dir.mkdir_p(File.join(HACKER_DIR, "plugins"))
  Dir.mkdir_p(File.dirname(HISTORY_FILE))
end

def display_welcome
  puts "#{COLOR_BOLD}#{COLOR_PURPLE}Welcome to Hacker Lang Interface (HLI) v#{VERSION}#{COLOR_RESET}"
  puts "#{COLOR_GRAY}Advanced scripting interface for HackerOS Linux system, inspired by Cargo#{COLOR_RESET}"
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
  if !File.exists?(RUNTIME_PATH)
    puts "#{COLOR_RED}Hacker runtime not found at #{RUNTIME_PATH}. Please install the Hacker Lang tools.#{COLOR_RESET}"
    return false
  end
  args = [file]
  args << "--verbose" if verbose
  status = Process.run(RUNTIME_PATH, args, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
  status.success?
end

def compile_command(file : String, output : String, verbose : Bool, bytes_mode : Bool) : Bool
  if !File.exists?(COMPILER_PATH)
    puts "#{COLOR_RED}Hacker compiler not found at #{COMPILER_PATH}. Please install the Hacker Lang tools.#{COLOR_RESET}"
    return false
  end
  args = [file, output]
  args << "--bytes" if bytes_mode
  args << "--verbose" if verbose
  status = Process.run(COMPILER_PATH, args, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
  status.success?
end

def check_command(file : String, verbose : Bool) : Bool
  if !File.exists?(PARSER_PATH)
    puts "#{COLOR_RED}Hacker parser not found at #{PARSER_PATH}. Please install the Hacker Lang tools.#{COLOR_RESET}"
    return false
  end
  args = [file]
  args << "--verbose" if verbose
  output = IO::Memory.new
  error_output = IO::Memory.new
  proc = Process.new(PARSER_PATH, args, output: output, error: error_output)
  status = proc.wait
  if !status.success?
    puts "#{COLOR_RED}Error parsing file: #{error_output.to_s}#{COLOR_RESET}"
    return false
  end
  begin
    parsed = JSON.parse(output.to_s)
    errors = parsed["errors"].as_a
    if errors.empty?
      puts "#{COLOR_GREEN}Syntax validation passed!#{COLOR_RESET}"
      true
    else
      puts "\n#{COLOR_RED}Errors:#{COLOR_RESET}"
      errors.each do |e|
        puts " #{COLOR_RED}✖ #{COLOR_RESET}#{e.as_s}"
      end
      puts ""
      false
    end
  rescue ex
    puts "#{COLOR_RED}Error unmarshaling parse output: #{ex.message}#{COLOR_RESET}"
    false
  end
end

def init_command(file : String?, verbose : Bool) : Bool
  target_file = file || "main.hacker"
  if File.exists?(target_file)
    puts "#{COLOR_RED}File #{target_file} already exists!#{COLOR_RESET}"
    return false
  end
  template = <<-TEMPLATE
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
>> echo "With var: $APP_NAME"
>>> long_running_command_with_vars
[
Author=Advanced User
Version=1.0
Description=System maintenance script
]
TEMPLATE
  File.write(target_file, template)
  puts "#{COLOR_GREEN}Initialized template at #{target_file}#{COLOR_RESET}"
  if verbose
    puts "\n#{COLOR_YELLOW}Template content:#{COLOR_RESET}"
    puts template.colorize(:yellow)
  end
  # Also create bytes.yaml if not exists
  bytes_file = "bytes.yaml"
  if !File.exists?(bytes_file)
    bytes_template = <<-YAML
package:
  name: my-hacker-project
  version: 0.1.0
  author: User
entry: #{target_file}
YAML
    File.write(bytes_file, bytes_template)
    puts "#{COLOR_GREEN}Initialized bytes.yaml for project#{COLOR_RESET}"
  end
  true
end

def clean_command(verbose : Bool) : Bool
  count = 0
  Dir.glob("/tmp/*.sh") do |path|
    if File.basename(path).starts_with?("tmp") || File.basename(path).starts_with?("sep_")
      puts "#{COLOR_YELLOW}Removed: #{path}#{COLOR_RESET}" if verbose
      File.delete(path)
      count += 1
    end
  end
  puts "#{COLOR_GREEN}Removed #{count} temporary files#{COLOR_RESET}"
  true
end

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
  url = "https://github.com/Bytes-Repository/Bytes-CLI-Tool/releases/download/v0.3/bytes"
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
@USER=admin
=2 > echo $USER
? [ -d /tmp ] > echo OK
& sleep 10
>> echo "With var: $USER"
>>> separate_command
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
  puts "- Library includes with # lib-name"
  puts "- Variables with @VAR=value"
  puts "- Loops with =N > command"
  puts "- Conditionals with ? condition > command"
  puts "- Background tasks with & command"
  puts "- Multi-line commands with >> and >>>"
  puts "- Metadata blocks with [ key=value ]"
  puts "\nFor more details, visit the official documentation or use 'hli tutorials' for examples."
  true
end

def tutorials_command : Bool
  puts "#{COLOR_BOLD}Hacker Lang Tutorials:#{COLOR_RESET}\n"
  puts "Tutorial 1: Basic Script"
  puts "Create a file main.hacker with:"
  puts "> echo 'Hello, Hacker Lang!'"
  puts "Run with: hli run"
  puts "\nTutorial 2: Using Libraries"
  puts "Add # logging to your script."
  puts "HLI will automatically install if missing."
  puts "\nTutorial 3: Projects"
  puts "Use 'hli init' to create a project with bytes.yaml."
  puts "Then 'hli run' to execute."
  true
end

def help_command(show_banner : Bool) : Bool
  if show_banner
    puts "#{COLOR_BOLD}#{COLOR_PURPLE}Hacker Lang Interface (HLI) - Advanced Scripting Tool v#{VERSION}#{COLOR_RESET}\n"
  end
  puts "#{COLOR_BOLD}Commands Overview:#{COLOR_RESET}"
  puts "#{"Command".ljust(15).colorize(:light_gray)} #{"Description".ljust(40).colorize(:light_gray)} #{"Arguments".ljust(40).colorize(:light_gray)}"
  commands = [
    ["run", "Execute a .hacker script or project", "[file] [--verbose]"],
    ["compile", "Compile to native executable or project", "[file] [-o output] [--verbose] [--bytes]"],
    ["check", "Validate syntax", "[file] [--verbose]"],
    ["init", "Generate template script/project", "[file] [--verbose]"],
    ["clean", "Remove temporary files", "[--verbose]"],
    ["repl", "Launch interactive REPL", "[--verbose]"],
    ["editor", "Launch hacker-editor", "[file]"],
    ["unpack", "Unpack and install bytes", "bytes [--verbose]"],
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
  puts "run: Execute script/project - Usage: hli run [file] [--verbose]"
  puts "compile: Compile to executable/project - Usage: hli compile [file] [-o output] [--verbose] [--bytes]"
  puts "check: Validate syntax - Usage: hli check [file] [--verbose]"
  puts "init: Generate template - Usage: hli init [file] [--verbose]"
  puts "clean: Remove temps - Usage: hli clean [--verbose]"
  puts "repl: Interactive REPL - Usage: hli repl [--verbose]"
  puts "editor: Launch editor - Usage: hli editor [file]"
  puts "unpack: Unpack and install bytes - Usage: hli unpack bytes [--verbose]"
  puts "docs: Show documentation - Usage: hli docs"
  puts "tutorials: Show tutorials - Usage: hli tutorials"
  puts "version: Show version - Usage: hli version"
  puts "help: Show help - Usage: hli help"
  puts "syntax: Show syntax examples - Usage: hli syntax"
  puts "help-ui: Interactive help UI - This UI"
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
  puts "#{COLOR_GREEN}Running project #{name} v#{version} by #{author}#{COLOR_RESET}"
  check_dependencies(entry, verbose)
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
  puts "#{COLOR_CYAN}Compiling project #{name} to #{output} with --bytes#{COLOR_RESET}"
  check_dependencies(entry, verbose)
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
  check_dependencies(entry, verbose)
  check_command(entry, verbose)
end

def check_dependencies(file : String, verbose : Bool) : Bool
  if !File.exists?(file)
    puts "#{COLOR_RED}File #{file} not found for dependency check.#{COLOR_RESET}"
    return false
  end
  content = File.read(file)
  libs_dir = File.join(HACKER_DIR, "libs")
  plugins_dir = File.join(HACKER_DIR, "plugins")
  missing_libs = [] of String
  missing_plugins = [] of String
  content.lines.each do |line|
    stripped = line.strip
    if stripped.starts_with?("//")
      plugin_name = stripped[2..].strip.split(" ").first?.try(&.gsub(/[^a-zA-Z0-9_-]/, ""))
      if plugin_name && plugin_name.size > 0 && !Dir.glob(File.join(plugins_dir, "#{plugin_name}*")).any?
        missing_plugins << plugin_name unless missing_plugins.includes?(plugin_name)
      end
    elsif stripped.starts_with?("#")
      lib_name = stripped[1..].strip.split(" ").first?.try(&.gsub(/[^a-zA-Z0-9_-]/, ""))
      if lib_name && lib_name.size > 0 && !Dir.glob(File.join(libs_dir, "#{lib_name}*")).any?
        missing_libs << lib_name unless missing_libs.includes?(lib_name)
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
      puts "#{COLOR_YELLOW}Installing lib #{l} via bytes...#{COLOR_RESET}"
      status = Process.run("bytes", ["install", l], output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
      return false unless status.success?
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
  # Handle global flags
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
  target : String? = nil
  case command
  when "run"
    parser = OptionParser.new do |p|
      p.banner = "#{COLOR_BOLD}Usage:#{COLOR_RESET} hli run [file] [options]\n\nExecute a .hacker script. No file assumes project from bytes.yaml."
      p.on("--verbose", "Enable verbose output") { verbose = true }
      p.on("-h", "--help", "Show this help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown option: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if ARGV.size > 1
      puts "#{COLOR_RED}Error: Expected at most one argument: [file]#{COLOR_RESET}"
      puts parser
      exit(1)
    elsif ARGV.size == 1
      file = ARGV.shift
    end
    if file.nil?
      entry = load_project_entry
      if entry.nil?
        puts "#{COLOR_RED}No project found. Use 'hli init' or specify a file.#{COLOR_RESET}"
        success = false
      else
        check_dependencies(entry, verbose)
        success = run_command(entry, verbose)
      end
    elsif file == "."
      success = run_bytes_project(verbose)
    else
      check_dependencies(file.not_nil!, verbose)
      success = run_command(file.not_nil!, verbose)
    end
  when "compile"
    parser = OptionParser.new do |p|
      p.banner = "#{COLOR_BOLD}Usage:#{COLOR_RESET} hli compile [file] [options]\n\nCompile to native executable. No file assumes project from bytes.yaml."
      p.on("-o OUTPUT", "--output OUTPUT", "Specify output file") { |o| output = o }
      p.on("--bytes", "Enable bytes mode") { bytes_mode = true }
      p.on("--verbose", "Enable verbose output") { verbose = true }
      p.on("-h", "--help", "Show this help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown option: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if ARGV.size > 1
      puts "#{COLOR_RED}Error: Expected at most one argument: [file]#{COLOR_RESET}"
      puts parser
      exit(1)
    elsif ARGV.size == 1
      file = ARGV.shift
    end
    if file.nil?
      entry = load_project_entry
      if entry.nil?
        puts "#{COLOR_RED}No project found. Use 'hli init' or specify a file.#{COLOR_RESET}"
        success = false
      else
        output = output.empty? ? File.basename(entry, File.extname(entry)) : output
        check_dependencies(entry, verbose)
        success = compile_command(entry, output, verbose, bytes_mode)
      end
    elsif file == "."
      success = compile_bytes_project(output, verbose)
    else
      output = output.empty? ? File.basename(file.not_nil!, File.extname(file.not_nil!)) : output
      check_dependencies(file.not_nil!, verbose)
      success = compile_command(file.not_nil!, output, verbose, bytes_mode)
    end
  when "check"
    parser = OptionParser.new do |p|
      p.banner = "#{COLOR_BOLD}Usage:#{COLOR_RESET} hli check [file] [options]\n\nValidate syntax of a .hacker file. No file assumes project from bytes.yaml."
      p.on("--verbose", "Enable verbose output") { verbose = true }
      p.on("-h", "--help", "Show this help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown option: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if ARGV.size > 1
      puts "#{COLOR_RED}Error: Expected at most one argument: [file]#{COLOR_RESET}"
      puts parser
      exit(1)
    elsif ARGV.size == 1
      file = ARGV.shift
    end
    if file.nil?
      entry = load_project_entry
      if entry.nil?
        puts "#{COLOR_RED}No project found. Use 'hli init' or specify a file.#{COLOR_RESET}"
        success = false
      else
        check_dependencies(entry, verbose)
        success = check_command(entry, verbose)
      end
    elsif file == "."
      success = check_bytes_project(verbose)
    else
      check_dependencies(file.not_nil!, verbose)
      success = check_command(file.not_nil!, verbose)
    end
  when "init"
    parser = OptionParser.new do |p|
      p.banner = "#{COLOR_BOLD}Usage:#{COLOR_RESET} hli init [file] [options]\n\nGenerate a template .hacker script/project."
      p.on("--verbose", "Enable verbose output (show template content)") { verbose = true }
      p.on("-h", "--help", "Show this help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown option: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if ARGV.size > 1
      puts "#{COLOR_RED}Error: Expected at most one argument: [file]#{COLOR_RESET}"
      puts parser
      exit(1)
    elsif ARGV.size == 1
      file = ARGV.shift
    end
    success = init_command(file, verbose)
  when "clean"
    parser = OptionParser.new do |p|
      p.banner = "#{COLOR_BOLD}Usage:#{COLOR_RESET} hli clean [options]\n\nRemove temporary files."
      p.on("--verbose", "Show removed files") { verbose = true }
      p.on("-h", "--help", "Show this help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown option: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if !ARGV.empty?
      puts "#{COLOR_RED}Error: No arguments expected#{COLOR_RESET}"
      puts parser
      exit(1)
    end
    success = clean_command(verbose)
  when "repl"
    parser = OptionParser.new do |p|
      p.banner = "#{COLOR_BOLD}Usage:#{COLOR_RESET} hli repl [options]\n\nLaunch interactive REPL."
      p.on("--verbose", "Enable verbose output") { verbose = true }
      p.on("-h", "--help", "Show this help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown option: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if !ARGV.empty?
      puts "#{COLOR_RED}Error: No arguments expected#{COLOR_RESET}"
      puts parser
      exit(1)
    end
    success = run_repl(verbose)
  when "editor"
    parser = OptionParser.new do |p|
      p.banner = "#{COLOR_BOLD}Usage:#{COLOR_RESET} hli editor [file] [options]\n\nLaunch hacker-editor."
      p.on("-h", "--help", "Show this help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown option: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if ARGV.size > 1
      puts "#{COLOR_RED}Error: Expected at most one argument: [file]#{COLOR_RESET}"
      puts parser
      exit(1)
    elsif ARGV.size == 1
      file = ARGV.shift
    end
    success = editor_command(file)
  when "unpack"
    parser = OptionParser.new do |p|
      p.banner = "#{COLOR_BOLD}Usage:#{COLOR_RESET} hli unpack bytes [options]\n\nUnpack and install bytes tool."
      p.on("--verbose", "Enable verbose output") { verbose = true }
      p.on("-h", "--help", "Show this help") { puts p; exit(0) }
      p.invalid_option do |opt|
        puts "#{COLOR_RED}Unknown option: #{opt}#{COLOR_RESET}"
        puts p
        exit(1)
      end
    end
    parser.parse(ARGV)
    if ARGV.size != 1
      puts "#{COLOR_RED}Error: Expected exactly one argument: bytes#{COLOR_RESET}"
      puts parser
      exit(1)
    end
    target = ARGV.shift
    if target == "bytes"
      success = unpack_bytes(verbose)
    else
      puts "#{COLOR_RED}Unknown unpack target: #{target} (only 'bytes' supported)#{COLOR_RESET}"
      success = false
    end
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
    puts "#{COLOR_YELLOW}Please use bytes #{command}#{COLOR_RESET}"
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
        puts "#{COLOR_RED}Error processing .hackerfile: #{ex.message}#{COLOR_RESET}"
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
