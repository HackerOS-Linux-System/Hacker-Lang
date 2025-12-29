require "option_parser"
require "file_utils"
require "json"
require "yaml"
require "regex"
require "process"
require "http/client"
require "colorize"
require "dir"
require "file"

VERSION = "1.2"

HOME = ENV["HOME"]
HACKER_DIR = File.join(HOME, ".hackeros", "hacker-lang")
BIN_DIR = File.join(HACKER_DIR, "bin")
HISTORY_FILE = File.join(HOME, ".hackeros", "history", "hacker_repl_history")
PARSER_PATH = File.join(BIN_DIR, "hacker-plsa")
COMPILER_PATH = File.join(BIN_DIR, "hacker-compiler")
RUNTIME_PATH = File.join(BIN_DIR, "hacker-runtime")
REPL_PATH = File.join(BIN_DIR, "hacker-repl")

class Config
  property name : String = ""
  property version : String = ""
  property author : String = ""
  property description : String = ""
  property entry : String = ""
  property libs : Hash(String, Array(String)) = Hash(String, Array(String)).new
  property scripts : Hash(String, String) = Hash(String, String).new
  property meta : Hash(String, String) = Hash(String, String).new
end

def ensure_hacker_dir
  FileUtils.mkdir_p(BIN_DIR)
  FileUtils.mkdir_p(File.join(HACKER_DIR, "libs"))
  FileUtils.mkdir_p(File.join(HACKER_DIR, "plugins"))
  FileUtils.mkdir_p(File.dirname(HISTORY_FILE))
end

def display_welcome
  puts "Welcome to Hacker Lang Projects (HLP) v#{VERSION}".colorize(:magenta)
  puts "Advanced scripting interface for HackerOS Linux system, inspired by Cargo".colorize(:light_gray)
  puts "Type 'hlp help' for commands or 'hlp repl' to start interactive mode.".colorize(:white)
  help_command(false)
end

def load_project_config : Config
  if File.exists?("bytes.yaml")
    data = YAML.parse(File.read("bytes.yaml")).as_h
    pkg = data["package"]?.try(&.as_h) || {} of String => YAML::Any
    config = Config.new
    config.name = pkg["name"]?.try(&.as_s) || ""
    config.version = pkg["version"]?.try(&.as_s) || ""
    config.author = pkg["author"]?.try(&.as_s) || ""
    config.entry = data["entry"]?.try(&.as_s) || ""
    return config
  elsif File.exists?("package.hfx")
    content = File.read("package.hfx")
    return parse_hfx(content)
  end
  raise "no project file found (bytes.yaml or package.hfx)"
end

def parse_hfx(content : String) : Config
  config = Config.new
  current_section = ""
  current_lang = ""
  lines = content.lines
  lines.each do |line|
    line = line.strip
    next if line.empty? || line.starts_with?("//")
    if line.ends_with?("{") || line.ends_with?("[")
      key = line.rchop("{[").strip
      case key
      when "package"
        current_section = "package"
      when "-> libs"
        current_section = "libs"
      when "-> scripts"
        current_section = "scripts"
      when "-> meta"
        current_section = "meta"
      end
      next
    end
    if line == "}" || line == "]"
      current_section = ""
      current_lang = ""
      next
    end
    if current_section == "libs"
      if line.starts_with?("-> ") && line.ends_with?(":")
        current_lang = line[3...-1].strip
        config.libs[current_lang] = [] of String
        next
      elsif line.starts_with?("-> ")
        lib = line[3..].strip
        if !current_lang.empty?
          config.libs[current_lang] << lib
        end
        next
      end
    end
    if ["scripts", "meta"].includes?(current_section) && line.starts_with?("-> ")
      subline = line[3..].strip
      if subline.includes?(":")
        parts = subline.split(":", 2)
        key = parts[0].strip
        value = parts[1].strip.rchop(",").strip('"')
        if current_section == "scripts"
          config.scripts[key] = value
        elsif current_section == "meta"
          config.meta[key] = value
        end
      end
      next
    end
    if line.includes?(":")
      parts = line.split(":", 2)
      key = parts[0].strip
      value = parts[1].strip.rchop(",").strip('"')
      if current_section == "package"
        case key
        when "name"
          config.name = value
        when "version"
          config.version = value
        when "author"
          config.author = value
        when "description"
          config.description = value
        end
      elsif key == "entry"
        config.entry = value
      end
    end
  end
  raise "missing entry in package.hfx" if config.entry.empty?
  config
end

def load_project_entry : String
  config = load_project_config
  config.entry
end

def run_command(file : String, verbose : Bool) : Bool
  if !File.exists?(RUNTIME_PATH)
    puts "Hacker runtime not found at #{RUNTIME_PATH}. Please install the Hacker Lang tools.".colorize(:red)
    return false
  end
  args = [file]
  args << "--verbose" if verbose
  status = Process.run(RUNTIME_PATH, args)
  status.success?
end

def compile_command(file : String, output : String, verbose : Bool, bytes_mode : Bool) : Bool
  if !File.exists?(COMPILER_PATH)
    puts "Hacker compiler not found at #{COMPILER_PATH}. Please install the Hacker Lang tools.".colorize(:red)
    return false
  end
  args = [file, output]
  args << "--bytes" if bytes_mode
  args << "--verbose" if verbose
  status = Process.run(COMPILER_PATH, args)
  status.success?
end

def check_command(file : String, verbose : Bool) : Bool
  if !File.exists?(PARSER_PATH)
    puts "Hacker parser not found at #{PARSER_PATH}. Please install the Hacker Lang tools.".colorize(:red)
    return false
  end
  args = [file]
  args << "--verbose" if verbose
  output = IO::Memory.new
  status = Process.run(PARSER_PATH, args, output: output, error: STDERR)
  if !status.success?
    puts "Error parsing file".colorize(:red)
    return false
  end
  begin
    parsed = JSON.parse(output.to_s).as_h
  rescue e : Exception
    puts "Error unmarshaling parse output: #{e.message}".colorize(:red)
    return false
  end
  errors = parsed["errors"]?.try(&.as_a) || [] of JSON::Any
  if errors.empty?
    puts "Syntax validation passed!".colorize(:green)
    return true
  end
  puts "Errors:".colorize(:red)
  errors.each do |e|
    puts "✖ #{e}".colorize(:red)
  end
  false
end

def init_command(file : String?, verbose : Bool) : Bool
  target_file = file || "main.hacker"
  if File.exists?(target_file)
    puts "File #{target_file} already exists!".colorize(:red)
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
echo "Starting update..."
sudo apt update && sudo apt upgrade -y ! System update
echo "With var: $APP_NAME"
long_running_command_with_vars
[
Author=Advanced User
Version=1.0
Description=System maintenance script
]
TEMPLATE
  File.write(target_file, template)
  puts "Initialized template at #{target_file}".colorize(:green)
  if verbose
    puts "Template content:".colorize(:yellow)
    puts template.colorize(:yellow)
  end
  bytes_file = "bytes.yaml"
  hfx_file = "package.hfx"
  if !File.exists?(bytes_file) && !File.exists?(hfx_file)
    hfx_template = <<-HFX
package {
name: "my-hacker-project",
version: "0.1.0",
author: "User",
description: "My Hacker project"
}
entry: "#{target_file}"
-> libs [
-> python:
-> library1
-> rust:
-> library2
]
-> scripts {
-> build: "hlp compile #{target_file}"
-> run: "hacker run ."
-> release: "hacker compile --bytes"
}
-> meta {
-> license: "MIT"
-> repo: "https://github.com/user/repo"
}
HFX
    File.write(hfx_file, hfx_template)
    puts "Initialized package.hfx for project".colorize(:green)
  end
  true
end

def clean_command(verbose : Bool) : Bool
  count = 0
  Dir.glob("/tmp/*.sh").each do |path|
    base = File.basename(path)
    if base.starts_with?("tmp") || base.starts_with?("sep_")
      if verbose
        puts "Removed: #{path}".colorize(:yellow)
      end
      File.delete(path)
      count += 1
    end
  end
  puts "Removed #{count} temporary files".colorize(:green)
  true
end

def unpack_bytes(verbose : Bool) : Bool
  bytes_path1 = File.join(BIN_DIR, "bytes")
  bytes_path2 = "/usr/bin/bytes"
  if File.exists?(bytes_path1)
    puts "Bytes already installed at #{bytes_path1}.".colorize(:green)
    return true
  end
  if File.exists?(bytes_path2)
    puts "Bytes already installed at #{bytes_path2}.".colorize(:green)
    return true
  end
  FileUtils.mkdir_p(BIN_DIR)
  url = "https://github.com/Bytes-Repository/Bytes-CLI-Tool/releases/download/v0.3/bytes"
  response = HTTP::Client.get(url)
  if response.status_code != 200
    puts "Error: status code #{response.status_code}".colorize(:red)
    return false
  end
  File.write(bytes_path1, response.body)
  File.chmod(bytes_path1, 0o755)
  if verbose
    puts "Downloaded and installed bytes from #{url} to #{bytes_path1}".colorize(:green)
  end
  puts "Bytes installed successfully!".colorize(:green)
  true
end

def run_repl(verbose : Bool) : Bool
  if !File.exists?(REPL_PATH)
    puts "Hacker REPL not found at #{REPL_PATH}. Please install the Hacker Lang tools.".colorize(:red)
    return false
  end
  args = [] of String
  args << "--verbose" if verbose
  status = Process.run(REPL_PATH, args, input: STDIN)
  if status.success?
    puts "REPL session ended.".colorize(:green)
    return true
  end
  puts "REPL failed.".colorize(:red)
  false
end

def version_command : Bool
  puts "Hacker Lang Projects (HLP) v#{VERSION}".colorize(:cyan)
  true
end

def syntax_command : Bool
  puts "Hacker Lang Syntax Example:".colorize.mode(:bold)
  example_code = <<-EXAMPLE
// sudo
# obsidian
@USER=admin
=2 > echo $USER
? [ -d /tmp ] > echo OK
& sleep 10
echo "With var: $USER"
separate_command
# logging
sudo apt update
[ Config=Example ]
EXAMPLE
  puts example_code.colorize(:white)
  true
end

def docs_command : Bool
  puts "Hacker Lang Documentation:".colorize.mode(:bold)
  puts "Hacker Lang is an advanced scripting language for HackerOS."
  puts "Key features:"
  features = [
    "Privileged operations with // sudo",
    "Library includes with # lib-name",
    "Variables with @VAR=value",
    "Loops with =N > command",
    "Conditionals with ? condition > command",
    "Background tasks with & command",
    "Multi-line commands with >> and >>>",
    "Metadata blocks with [ key=value ]",
  ]
  features.each { |f| puts "- #{f}" }
  puts "\nFor more details, visit the official documentation or use 'hlp tutorials' for examples."
  true
end

def tutorials_command : Bool
  puts "Hacker Lang Tutorials:".colorize.mode(:bold)
  puts "Tutorial 1: Basic Script"
  puts "Create a file main.hacker with:"
  puts "> echo 'Hello, Hacker Lang!'"
  puts "Run with: hlp run"
  puts "\nTutorial 2: Using Libraries"
  puts "Add # logging to your script."
  puts "HLP will automatically install if missing."
  puts "\nTutorial 3: Projects"
  puts "Use 'hlp init' to create a project with bytes.yaml."
  puts "Then 'hlp run' to execute."
  true
end

def help_command(show_banner : Bool) : Bool
  if show_banner
    puts "Hacker Lang Projects (HLP) - Advanced Scripting Tool v#{VERSION}".colorize(:magenta).mode(:bold)
  end
  puts "Commands Overview:".colorize.mode(:bold)
  # Manual table formatting since no tablo shard
  puts "Command".ljust(15) + "Description".ljust(50) + "Arguments"
  puts "-" * 80
  [
    ["run", "Execute a .hacker script or project", "[file] [--verbose]"],
    ["compile", "Compile to native executable or project", "[file] [-o output] [--verbose] [--bytes]"],
    ["check", "Validate syntax", "[file] [--verbose]"],
    ["init", "Generate template script/project", "[file] [--verbose]"],
    ["clean", "Remove temporary files", "[--verbose]"],
    ["repl", "Launch interactive REPL", "[--verbose]"],
    ["unpack", "Unpack and install bytes", "bytes [--verbose]"],
    ["docs", "Show documentation", ""],
    ["tutorials", "Show tutorials", ""],
    ["version", "Display version", ""],
    ["help", "Show this help menu", ""],
    ["syntax", "Show syntax examples", ""],
    ["help-ui", "Show special commands list", ""],
  ].each do |row|
    puts row[0].ljust(15) + row[1].ljust(50) + row[2]
  end
  true
end

def run_help_ui : Bool
  puts "Hacker Lang Commands List".colorize(:magenta).mode(:bold)
  items = [
    "run: Execute script/project - Usage: hlp run [file] [--verbose]",
    "compile: Compile to executable/project - Usage: hlp compile [file] [-o output] [--verbose] [--bytes]",
    "check: Validate syntax - Usage: hlp check [file] [--verbose]",
    "init: Generate template - Usage: hlp init [file] [--verbose]",
    "clean: Remove temps - Usage: hlp clean [--verbose]",
    "repl: Interactive REPL - Usage: hlp repl [--verbose]",
    "unpack: Unpack and install bytes - Usage: hlp unpack bytes [--verbose]",
    "docs: Show documentation - Usage: hlp docs",
    "tutorials: Show tutorials - Usage: hlp tutorials",
    "version: Show version - Usage: hlp version",
    "help: Show help - Usage: hlp help",
    "syntax: Show syntax examples - Usage: hlp syntax",
    "help-ui: Interactive help UI - This UI",
  ]
  items.each { |item| puts "- #{item}".colorize(:magenta) }
  true
end

def run_project(verbose : Bool) : Bool
  begin
    config = load_project_config
  rescue e : Exception
    puts "#{e.message}. Use 'hlp init' to create a project.".colorize(:red)
    return false
  end
  puts "Running project #{config.name} v#{config.version} by #{config.author}".colorize(:green)
  check_dependencies(config.entry, verbose)
  run_command(config.entry, verbose)
end

def compile_project(output : String?, verbose : Bool, bytes_mode : Bool) : Bool
  begin
    config = load_project_config
  rescue e : Exception
    puts "#{e.message}. Use 'hlp init' to create a project.".colorize(:red)
    return false
  end
  output ||= config.name
  puts "Compiling project #{config.name} to #{output} with --bytes".colorize(:cyan)
  check_dependencies(config.entry, verbose)
  compile_command(config.entry, output, verbose, bytes_mode)
end

def check_project(verbose : Bool) : Bool
  begin
    config = load_project_config
  rescue e : Exception
    puts "#{e.message}. Use 'hlp init' to create a project.".colorize(:red)
    return false
  end
  check_dependencies(config.entry, verbose)
  check_command(config.entry, verbose)
end

def check_dependencies(file : String, verbose : Bool) : Bool
  return false if !File.exists?(file)
  content = File.read(file)
  libs_dir = File.join(HACKER_DIR, "libs")
  plugins_dir = File.join(HACKER_DIR, "plugins")
  missing_libs = [] of String
  missing_plugins = [] of String
  content.lines.each do |line|
    stripped = line.strip
    next if stripped.empty?
    if stripped.starts_with?("//")
      plugin_name = stripped[2..].split.first.gsub(/[^a-zA-Z0-9_-]/, "")
      next if plugin_name.empty? || !Dir.glob(File.join(plugins_dir, plugin_name + "*")).empty? || missing_plugins.includes?(plugin_name)
      missing_plugins << plugin_name
    elsif stripped.starts_with?("#")
      lib_name = stripped[1..].split.first.gsub(/[^a-zA-Z0-9_-]/, "")
      next if lib_name.empty? || !Dir.glob(File.join(libs_dir, lib_name + "*")).empty? || missing_libs.includes?(lib_name)
      missing_libs << lib_name
    end
  end
  if !missing_plugins.empty?
    if verbose
      puts "Missing plugins: #{missing_plugins.join(", ")}".colorize(:yellow)
    end
    missing_plugins.each do |p|
      puts "Installing plugin #{p} via bytes...".colorize(:yellow)
      status = Process.run("bytes", ["plugin", "install", p])
      return false if !status.success?
    end
  end
  if !missing_libs.empty?
    if verbose
      puts "Missing libs: #{missing_libs.join(", ")}".colorize(:yellow)
    end
    missing_libs.each do |l|
      puts "Installing lib #{l} via bytes...".colorize(:yellow)
      status = Process.run("bytes", ["install", l])
      return false if !status.success?
    end
  end
  true
end

class TaskConfig
  property vars : Hash(String, String | Int32 | Float64)
  property tasks : Hash(String, Hash(String, Array(String)))
  property aliases : Hash(String, String)

  def initialize(@vars = Hash(String, String | Int32 | Float64).new, @tasks = Hash(String, Hash(String, Array(String))).new, @aliases = Hash(String, String).new)
  end
end

def execute_task(task_name : String, config : TaskConfig, executed = Set(String).new)
  raise "cycle detected in tasks involving #{task_name}" if executed.includes?(task_name)
  executed.add(task_name)
  raise "task #{task_name} not found" if !config.tasks.has_key?(task_name)
  task = config.tasks[task_name]
  (task["requires"]? || [] of String).each { |req| execute_task(req, config, executed) }
  (task["run"]? || [] of String).each do |cmd_str|
    config.vars.each do |var_name, var_value|
      cmd_str = cmd_str.gsub("{{#{var_name}}}", var_value.to_s)
    end
    status = Process.run("sh", ["-c", cmd_str])
    raise "command failed: #{cmd_str}" if !status.success?
  end
end

# Main execution
ensure_hacker_dir

if ARGV.size > 0
  command = ARGV[0]
  if ["--version", "-v"].includes?(command)
    version_command
    exit 0
  elsif ["--help", "-h"].includes?(command)
    help_command(true)
    exit 0
  end
end

# Manual argument parsing since subparsers are complex; use OptionParser for each command
command = ARGV.shift? || ""
success = true

case command
when ""
  display_welcome
  exit 0
when "run"
  verbose = ARGV.includes?("--verbose")
  file = ARGV.reject { |a| a == "--verbose" }.first? || nil
  if file.nil?
    begin
      entry = load_project_entry
      check_dependencies(entry, verbose)
      success = run_command(entry, verbose)
    rescue e : Exception
      puts "No project found. Use 'hlp init' or specify a file.".colorize(:red)
      success = false
    end
  elsif file == "."
    success = run_project(verbose)
  else
    check_dependencies(file, verbose)
    success = run_command(file, verbose)
  end
when "compile"
  verbose = ARGV.includes?("--verbose")
  bytes_mode = ARGV.includes?("--bytes")
  output_index = ARGV.index("-o")
  output = if output_index
             ARGV[output_index + 1]?
           else
             nil
           end
  file = ARGV.reject { |a| ["--verbose", "--bytes", "-o", output].includes?(a) }.first? || nil
  if file.nil?
    begin
      entry = load_project_entry
      output ||= File.basename(entry, File.extname(entry))
      check_dependencies(entry, verbose)
      success = compile_command(entry, output, verbose, bytes_mode)
    rescue e : Exception
      puts "No project found. Use 'hlp init' or specify a file.".colorize(:red)
      success = false
    end
  elsif file == "."
    success = compile_project(output, verbose, bytes_mode)
  else
    output ||= File.basename(file, File.extname(file))
    check_dependencies(file, verbose)
    success = compile_command(file, output, verbose, bytes_mode)
  end
when "check"
  verbose = ARGV.includes?("--verbose")
  file = ARGV.reject { |a| a == "--verbose" }.first? || nil
  if file.nil?
    begin
      entry = load_project_entry
      check_dependencies(entry, verbose)
      success = check_command(entry, verbose)
    rescue e : Exception
      puts "No project found. Use 'hlp init' or specify a file.".colorize(:red)
      success = false
    end
  elsif file == "."
    success = check_project(verbose)
  else
    check_dependencies(file, verbose)
    success = check_command(file, verbose)
  end
when "init"
  verbose = ARGV.includes?("--verbose")
  file = ARGV.reject { |a| a == "--verbose" }.first? || nil
  success = init_command(file, verbose)
when "clean"
  verbose = ARGV.includes?("--verbose")
  success = clean_command(verbose)
when "repl"
  verbose = ARGV.includes?("--verbose")
  success = run_repl(verbose)
when "unpack"
  verbose = ARGV.includes?("--verbose")
  item = ARGV.reject { |a| a == "--verbose" }.first? || ""
  if item != "bytes"
    puts "Expected exactly one argument: bytes".colorize(:red)
    success = false
  else
    success = unpack_bytes(verbose)
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
else
  if File.exists?(".hackerfile")
    data = YAML.parse(File.read(".hackerfile")).as_h
    vars = data["vars"]?.try(&.as_h) || {} of String => YAML::Any
    vars_hash = vars.transform_values(&.as(String | Int64 | Float64))
    tasks = data["tasks"]?.try(&.as_h) || {} of String => YAML::Any
    tasks_hash = tasks.transform_values { |v| v.as_h.transform_values { |vv| vv.as_a.map(&.as_s) } }
    aliases = data["aliases"]?.try(&.as_h.transform_values(&.as_s)) || {} of String => String
    config = TaskConfig.new(vars_hash, tasks_hash, aliases)
    aliased_task = config.aliases[command]? || command
    if config.tasks.has_key?(aliased_task)
      begin
        execute_task(aliased_task, config)
        exit 0
      rescue e : Exception
        puts "Error executing task: #{e.message}".colorize(:red)
        exit 1
      end
    else
      puts "Unknown task: #{command}".colorize(:red)
      help_command(false)
      exit 1
    end
  elsif ["install", "update", "remove"].includes?(command)
    puts "Please use bytes #{command}".colorize(:yellow)
    exit 0
  else
    puts "Unknown command: #{command}".colorize(:red)
    help_command(false)
    exit 1
  end
end

exit success ? 0 : 1
