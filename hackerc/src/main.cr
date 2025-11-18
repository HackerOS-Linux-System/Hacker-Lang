require "file_utils"
require "http/client"
require "process"
require "yaml"
VERSION = "0.1.1"
HACKER_DIR = Path.home / ".hackeros" / "hacker-lang"
BIN_DIR = HACKER_DIR / "bin"
LIBS_DIR = HACKER_DIR / "libs"
HISTORY_DIR = Path.home / ".hackeros" / "history"
HISTORY_FILE = HISTORY_DIR / "hacker_repl_history"
TITLE_STYLE = "\e[1;4;35m"
HEADER_STYLE = "\e[1;33m"
EXAMPLE_STYLE = "\e[36m"
SUCCESS_STYLE = "\e[1;3;32m"
ERROR_STYLE = "\e[1;4;31m"
WARNING_STYLE = "\e[1;33m"
INFO_STYLE = "\e[4;34m"
PROMPT_STYLE = "\e[1;35m"
HIGHLIGHT_STYLE = "\e[42;37m"
RESET = "\e[0m"
def colored(text : String, style : String) : String
  "#{style}#{text}#{RESET}"
end
def strip_ansi(s : String) : String
  s.gsub(/\e\[[\d;]*[a-zA-Z]/, "")
end
def print_panel(content : String, title : String, title_style : String, border_style : String)
  content_lines = content.lines
  content_max = content_lines.map { |l| strip_ansi(l).size }.max? || 0
  title_colored = colored(title, title_style)
  title_stripped = strip_ansi(title_colored)
  title_min = title_stripped.size + 4
  content_min = content_max + 4
  box_width = [content_min, title_min].max
  inner_width = box_width - 2
  content_width = inner_width - 2
  fill_length = box_width - title_stripped.size - 4
  fill = "─" * [0, fill_length].max
  top = "#{border_style}┌─#{RESET}#{title_colored}#{border_style}─#{fill}┐#{RESET}"
  bottom = "#{border_style}└#{ "─" * (box_width - 2) }┘#{RESET}"
  puts top
  if content_lines.empty?
    pad = " " * content_width
    puts "#{border_style}│#{pad}│#{RESET}"
  else
    content_lines.each do |line|
      line_stripped = strip_ansi(line)
      pad = " " * (content_width - line_stripped.size)
      puts "#{border_style}│ #{RESET}#{line}#{pad}#{border_style} │#{RESET}"
    end
  end
  puts bottom
end
def parse_lines(lines : Array(String), verbose : Bool = false) : Hash(String, (Array(String) | Hash(String, String)))
  deps = [] of String
  libs = [] of String
  vars_dict = Hash(String, String).new
  cmds = [] of String
  includes = [] of String
  binaries = [] of String
  plugins = [] of String
  errors = [] of String
  config = Hash(String, String).new
  in_config = false
  lines.each_with_index do |line, index|
    line = line.strip
    next if line.empty? || line.starts_with?("!")
    line_num = index + 1
    if line == "["
      if in_config
        errors << "Line #{line_num}: Nested config block detected"
      end
      in_config = true
      next
    end
    if line == "]"
      if !in_config
        errors << "Line #{line_num}: Unmatched closing bracket"
      end
      in_config = false
      next
    end
    if in_config
      if line.includes?("=")
        k, v = line.split("=", limit = 2)
        config[k.strip] = v.strip
      else
        errors << "Line #{line_num}: Invalid configuration entry: #{line}"
      end
      next
    end
    if line.starts_with?("//")
      deps.concat line[2..].strip.split
    elsif line.starts_with?("#")
      lib_name = line[1..].strip
      if !lib_name.empty?
        lib_path = LIBS_DIR / lib_name / "main.hacker"
        if File.exists?(lib_path)
          includes << lib_name
        else
          libs << lib_name
        end
      end
    elsif line.starts_with?("@")
      var_def = line[1..].strip
      if var_def.includes?("=")
        k, v = var_def.split("=", limit = 2)
        vars_dict[k.strip] = v.strip
      else
        errors << "Line #{line_num}: Invalid variable definition: #{line}"
      end
    elsif line.starts_with?("=")
      parts = line[1..].strip.split(">", limit = 2)
      if parts.size == 2
        begin
          n = parts[0].strip.to_i
          cmd = parts[1].strip
          loop_cmd = "for i in $(seq 1 #{n}); do #{cmd}; done"
          cmds << loop_cmd
        rescue
          errors << "Line #{line_num}: Invalid loop count in: #{line}"
        end
      else
        errors << "Line #{line_num}: Invalid loop syntax: #{line}"
      end
    elsif line.starts_with?("?")
      parts = line[1..].strip.split(">", limit = 2)
      if parts.size == 2
        cond = parts[0].strip
        cmd = parts[1].strip
        if_cmd = "if #{cond}; then #{cmd}; fi"
        cmds << if_cmd
      else
        errors << "Line #{line_num}: Invalid conditional syntax: #{line}"
      end
    elsif line.starts_with?("&")
      plugin = line[1..].strip
      plugins << plugin
    elsif line.starts_with?(">")
      cmd = line[1..].strip
      cmds << cmd
    elsif line.starts_with?("%")
      binary = line[1..].strip
      binaries << binary
    else
      cmds << line
    end
  end
  {
    "deps" => deps.uniq,
    "libs" => libs,
    "vars" => vars_dict,
    "cmds" => cmds,
    "includes" => includes,
    "binaries" => binaries,
    "plugins" => plugins,
    "errors" => errors,
    "config" => config,
  }
end
def run_command(file : String, verbose : Bool) : Bool
  tmp_path : String? = nil
  begin
    lines = File.read_lines(file)
    parsed = parse_lines(lines, verbose)
    errors_arr = parsed["errors"].as(Array(String))
    if !errors_arr.empty?
      print_panel(errors_arr.join("\n"), "Syntax Errors", ERROR_STYLE, "\e[31m")
      return false
    end
    libs_arr = parsed["libs"].as(Array(String))
    if !libs_arr.empty?
      puts colored("Warning: Missing custom libraries: #{libs_arr.join(", ")}", WARNING_STYLE)
      puts colored("Install them using: bytes install <lib>", WARNING_STYLE)
    end
    tmp_path = "/tmp/hackerc_#{Process.pid}_#{Random.rand(10000)}.sh"
    File.open(tmp_path, "w") do |temp|
      temp.puts "#!/bin/bash"
      temp.puts "set -e"
      temp.puts "set -u"
      parsed["vars"].as(Hash(String, String)).each do |k, v|
        temp.puts "export #{k}=\"#{v}\""
      end
      parsed["deps"].as(Array(String)).each do |dep|
        if dep != "sudo"
          temp.puts "command -v #{dep} &> /dev/null || (sudo apt update && sudo apt install -y #{dep})"
        end
      end
      parsed["includes"].as(Array(String)).each do |inc|
        lib_path = LIBS_DIR / inc / "main.hacker"
        temp.puts "# Included from library: #{inc}"
        temp.puts File.read(lib_path)
        temp.puts
      end
      parsed["cmds"].as(Array(String)).each do |cmd|
        temp.puts cmd
      end
      parsed["binaries"].as(Array(String)).each do |bin|
        temp.puts bin
      end
      parsed["plugins"].as(Array(String)).each do |plugin|
        temp.puts "#{plugin} &"
      end
    end
    File.chmod(tmp_path, 0o755)
    puts colored("Executing script file: #{file}", INFO_STYLE)
    puts colored("Configuration: #{parsed["config"]}", INFO_STYLE)
    puts colored("Starting execution...", SUCCESS_STYLE)
    puts "Running script commands..."
    env = ENV.to_h
    env.merge! parsed["vars"].as(Hash(String, String))
    status = Process.run("bash", args: [tmp_path], env: env, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
    File.delete(tmp_path)
    puts "Completed"
    if !status.success?
      puts colored("Execution encountered an error", ERROR_STYLE)
      return false
    end
    puts colored("Execution completed successfully", SUCCESS_STYLE)
    return true
  rescue ex
    puts colored("Error during execution: #{ex.message}", ERROR_STYLE)
    if tmp_path && File.exists?(tmp_path)
      File.delete(tmp_path)
    end
    false
  end
end
def compile_command(file : String, output : String, verbose : Bool) : Bool
  puts colored("Compiling file #{file} to output #{output} (bash executable)", INFO_STYLE)
  begin
    lines = File.read_lines(file)
    parsed = parse_lines(lines, verbose)
    if !parsed["errors"].as(Array(String)).empty?
      print_panel(parsed["errors"].as(Array(String)).join("\n"), "Syntax Errors", ERROR_STYLE, "\e[31m")
      return false
    end
    File.open(output, "w") do |io|
      io.puts "#!/bin/bash"
      io.puts "set -e"
      io.puts "set -u"
      parsed["vars"].as(Hash(String, String)).each do |k, v|
        io.puts "export #{k}=\"#{v}\""
      end
      parsed["deps"].as(Array(String)).each do |dep|
        if dep != "sudo"
          io.puts "command -v #{dep} &> /dev/null || (sudo apt update && sudo apt install -y #{dep})"
        end
      end
      parsed["includes"].as(Array(String)).each do |inc|
        lib_path = LIBS_DIR / inc / "main.hacker"
        io.puts "# Included from library: #{inc}"
        io.puts File.read(lib_path)
        io.puts
      end
      parsed["cmds"].as(Array(String)).each do |cmd|
        io.puts cmd
      end
      parsed["binaries"].as(Array(String)).each do |bin|
        io.puts bin
      end
      parsed["plugins"].as(Array(String)).each do |plugin|
        io.puts "#{plugin} &"
      end
    end
    File.chmod(output, 0o755)
    puts colored("Compilation process completed successfully", SUCCESS_STYLE)
    true
  rescue ex
    puts colored("Compilation error: #{ex.message}", ERROR_STYLE)
    false
  end
end
def check_command(file : String, verbose : Bool) : Bool
  begin
    lines = File.read_lines(file)
    parsed = parse_lines(lines, verbose)
    if !parsed["errors"].as(Array(String)).empty?
      print_panel(parsed["errors"].as(Array(String)).join("\n"), "Syntax Errors", ERROR_STYLE, "\e[31m")
      false
    else
      puts colored("Syntax check passed without issues", SUCCESS_STYLE)
      true
    end
  rescue ex
    puts colored("Error during check: #{ex.message}", ERROR_STYLE)
    false
  end
end
def init_command(file : String, verbose : Bool) : Bool
  if File.exists?(file)
    puts colored("File #{file} already exists", ERROR_STYLE)
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
% some_binary_command ! Example binary inclusion
[
Author=Advanced User
Version=1.0
Description=System maintenance script
]
TEMPLATE
  begin
    File.write(file, template)
    puts colored("Template initialized at #{file}", SUCCESS_STYLE)
    if verbose
      print_panel(template, "Template Content", EXAMPLE_STYLE, "\e[36m")
    end
    true
  rescue ex
    puts colored("Initialization error: #{ex.message}", ERROR_STYLE)
    false
  end
end
def clean_command(verbose : Bool) : Bool
  temp_dir = Dir.tempdir
  count = 0
  Dir.glob("#{temp_dir}/*.sh") do |f|
    if Path.new(f).basename.starts_with?("tmp")
      File.delete(f)
      count += 1
      if verbose
        puts colored("Removed temporary file: #{f}", WARNING_STYLE)
      end
    end
  end
  puts colored("Cleaned #{count} temporary files", SUCCESS_STYLE)
  true
end
def unpack_bytes(verbose : Bool) : Bool
  bytes_path1 = HACKER_DIR / "bin" / "bytes"
  bytes_path2 = Path.new("/usr/bin/bytes")
  if File.exists?(bytes_path1)
    puts colored("Bytes tool already installed at #{bytes_path1}", SUCCESS_STYLE)
    return true
  end
  if File.exists?(bytes_path2)
    puts colored("Bytes tool already installed at #{bytes_path2}", SUCCESS_STYLE)
    return true
  end
  url = "https://github.com/Bytes-Repository/Bytes-CLI-Tool/releases/download/v0.4/bytes"
  begin
    response = HTTP::Client.get(url)
    if response.status_code != 200
      puts colored("Download error: status code #{response.status_code}", ERROR_STYLE)
      return false
    end
    Dir.mkdir_p(bytes_path1.dirname)
    File.write(bytes_path1, response.body)
    File.chmod(bytes_path1, 0o755)
    if verbose
      puts colored("Downloaded and installed bytes from #{url} to #{bytes_path1}", SUCCESS_STYLE)
    end
    puts colored("Bytes installation completed successfully", SUCCESS_STYLE)
    true
  rescue ex
    puts colored("Error installing bytes: #{ex.message}", ERROR_STYLE)
    false
  end
end
def editor_command(file : String? = nil) : Bool
  editor_path = HACKER_DIR / "bin" / "hacker-editor.AppImage"
  if !File.exists?(editor_path)
    puts colored("Editor not found at #{editor_path}. Please ensure it is installed.", ERROR_STYLE)
    return false
  end
  args = [editor_path.to_s]
  file.try { |f| args << f }
  puts colored("Launching editor with arguments: #{args.join(" ")}", INFO_STYLE)
  begin
    status = Process.run(args[0], args: args[1..]? || [] of String, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
    puts colored("Editor session has completed", SUCCESS_STYLE)
    status.success?
  rescue ex
    puts colored("Editor launch failed: #{ex.message}", ERROR_STYLE)
    false
  end
end
def run_repl(verbose : Bool) : Bool
  repl_path = BIN_DIR / "hackerc" / "repl"
  args = [repl_path.to_s]
  if verbose
    args << "--verbose"
  end
  puts colored("Starting REPL session...", INFO_STYLE)
  begin
    status = Process.run(args[0], args: args[1..]? || [] of String, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
    puts colored("REPL session has ended", SUCCESS_STYLE)
    status.success?
  rescue ex
    puts colored("REPL error: #{ex.message}", ERROR_STYLE)
    false
  end
end
def run_help_ui : Bool
  help_ui_path = BIN_DIR / "hackerc" / "help-ui"
  puts colored("Launching Help UI interface...", INFO_STYLE)
  begin
    status = Process.run(help_ui_path.to_s, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
    status.success?
  rescue ex
    puts colored("Help UI error: #{ex.message}", ERROR_STYLE)
    false
  end
end
def version_command : Bool
  print_panel("Hacker Lang CLI version #{VERSION}\nEnhanced edition with expanded features", "Version Information", INFO_STYLE, "\e[34m")
  true
end
def help_command(show_banner : Bool = true) : Bool
  if show_banner
    print_panel("Hacker Lang CLI - Advanced Scripting Tool", "Help Menu", TITLE_STYLE, "\e[33m")
  end
  puts colored("Commands Overview", HEADER_STYLE)
  table_data = [
    ["Command", "Description", "Arguments"],
    ["run", "Execute a .hacker script", "file [--verbose] or . for bytes project"],
    ["compile", "Compile to native executable", "file [-o output] [--verbose] [--bytes]"],
    ["check", "Validate syntax", "file [--verbose]"],
    ["init", "Generate template script", "file [--verbose]"],
    ["clean", "Remove temporary files", "[--verbose]"],
    ["repl", "Launch interactive REPL", "[--verbose]"],
    ["editor", "Launch hacker-editor", "[file]"],
    ["unpack", "Unpack and install bytes", "bytes [--verbose]"],
    ["version", "Display version", ""],
    ["help", "Show this help menu", ""],
    ["help-ui", "Show special commands list", ""],
  ]
  col_widths = [0, 0, 0]
  table_data.each do |row|
    row.each_with_index do |cell, i|
      col_widths[i] = [col_widths[i], strip_ansi(cell).size].max
    end
  end
  header = table_data[0].map_with_index { |h, i| colored(h.ljust(col_widths[i]), "\e[1;35m") }.join(" | ")
  puts header
  sep = col_widths.map { |w| "─" * w }.join("-+-")
  puts sep
  table_data[1..].each do |row|
    line = row.map_with_index do |cell, i|
      color = case i
              when 0 then "\e[1;32m"
              when 1 then "\e[1;36m"
              else "\e[1;33m"
              end
      colored(cell.ljust(col_widths[i]), color)
    end.join(" | ")
    puts line
  end
  puts "\n" + colored("Syntax Example:", HEADER_STYLE)
  example_code = <<-EXAMPLE
// sudo
# obsidian
@USER=admin
=2 > echo $USER
? [ -d /tmp ] > echo OK
& sleep 10
# logging
> sudo apt update
[
Config=Example
]
EXAMPLE
  print_panel(example_code, "Example Script", EXAMPLE_STYLE, "\e[36m")
  true
end
def display_welcome
  print_panel("Welcome to Hacker Lang CLI v#{VERSION}\nAdvanced scripting tool for HackerOS\n", "Hacker Lang", TITLE_STYLE, "\e[1;32m")
  puts colored("Type 'hackerc help' for a list of commands or 'hackerc repl' to enter interactive mode.", INFO_STYLE)
  puts colored("Loading help overview...", INFO_STYLE)
  help_command(false)
  puts colored("\nSystem ready for commands.", SUCCESS_STYLE)
end
def run_bytes_project(verbose : Bool) : Bool
  bytes_file = "hacker.bytes"
  begin
    content = File.read(bytes_file)
    project = YAML.parse(content).as_h
    package = project["package"].as_h
    puts colored("Running project #{package["name"]} version #{package["version"]} by #{package["author"]}", SUCCESS_STYLE)
    run_command(project["entry"].as_s, verbose)
  rescue ex
    puts colored("Project error: #{ex.message}", ERROR_STYLE)
    false
  end
end
def compile_bytes_project(output : String, verbose : Bool) : Bool
  bytes_file = "hacker.bytes"
  begin
    content = File.read(bytes_file)
    project = YAML.parse(content).as_h
    if output.empty?
      output = project["package"].as_h["name"].as_s
    end
    compile_command(project["entry"].as_s, output, verbose)
  rescue ex
    puts colored("Project compilation error: #{ex.message}", ERROR_STYLE)
    false
  end
end
def ensure_hacker_dir
  Dir.mkdir_p(HACKER_DIR.to_s)
  Dir.mkdir_p(BIN_DIR.to_s)
  Dir.mkdir_p(LIBS_DIR.to_s)
  Dir.mkdir_p(HISTORY_DIR.to_s)
end
def main
  ensure_hacker_dir
  if ARGV.empty?
    display_welcome
    exit 0
  end
  command = ARGV[0]
  case command
  when "run"
    verbose = false
    file = ""
    i = 1
    while i < ARGV.size
      arg = ARGV[i]
      if arg == "--verbose"
        verbose = true
      elsif file.empty?
        file = arg
      else
        puts colored("Unknown argument: #{arg}", ERROR_STYLE)
        exit 1
      end
      i += 1
    end
    if file.empty?
      puts colored("Missing file argument", ERROR_STYLE)
      exit 1
    end
    success = file == "." ? run_bytes_project(verbose) : run_command(file, verbose)
    exit success ? 0 : 1
  when "compile"
    verbose = false
    bytes_mode = false
    output = ""
    file = ""
    i = 1
    while i < ARGV.size
      arg = ARGV[i]
      if arg == "--verbose"
        verbose = true
      elsif arg == "--bytes"
        bytes_mode = true
      elsif arg == "-o"
        i += 1
        if i < ARGV.size
          output = ARGV[i]
        else
          puts colored("Missing output after -o", ERROR_STYLE)
          exit 1
        end
      elsif file.empty?
        file = arg
      else
        puts colored("Unknown argument: #{arg}", ERROR_STYLE)
        exit 1
      end
      i += 1
    end
    if file.empty? && !bytes_mode
      puts colored("Missing file argument", ERROR_STYLE)
      exit 1
    end
    if output.empty?
      output = bytes_mode ? "" : Path.new(file).stem
    end
    success = bytes_mode ? compile_bytes_project(output, verbose) : compile_command(file, output, verbose)
    exit success ? 0 : 1
  when "check"
    verbose = false
    file = ""
    i = 1
    while i < ARGV.size
      arg = ARGV[i]
      if arg == "--verbose"
        verbose = true
      elsif file.empty?
        file = arg
      else
        puts colored("Unknown argument: #{arg}", ERROR_STYLE)
        exit 1
      end
      i += 1
    end
    if file.empty?
      puts colored("Missing file argument", ERROR_STYLE)
      exit 1
    end
    success = check_command(file, verbose)
    exit success ? 0 : 1
  when "init"
    verbose = false
    file = ""
    i = 1
    while i < ARGV.size
      arg = ARGV[i]
      if arg == "--verbose"
        verbose = true
      elsif file.empty?
        file = arg
      else
        puts colored("Unknown argument: #{arg}", ERROR_STYLE)
        exit 1
      end
      i += 1
    end
    if file.empty?
      puts colored("Missing file argument", ERROR_STYLE)
      exit 1
    end
    success = init_command(file, verbose)
    exit success ? 0 : 1
  when "clean"
    verbose = false
    i = 1
    while i < ARGV.size
      if ARGV[i] == "--verbose"
        verbose = true
      else
        puts colored("Unknown argument: #{ARGV[i]}", ERROR_STYLE)
        exit 1
      end
      i += 1
    end
    success = clean_command(verbose)
    exit success ? 0 : 1
  when "repl"
    verbose = false
    i = 1
    while i < ARGV.size
      if ARGV[i] == "--verbose"
        verbose = true
      else
        puts colored("Unknown argument: #{ARGV[i]}", ERROR_STYLE)
        exit 1
      end
      i += 1
    end
    success = run_repl(verbose)
    exit success ? 0 : 1
  when "editor"
    file = nil.as(String?)
    i = 1
    while i < ARGV.size
      if file.nil?
        file = ARGV[i]
      else
        puts colored("Unknown argument: #{ARGV[i]}", ERROR_STYLE)
        exit 1
      end
      i += 1
    end
    success = editor_command(file)
    exit success ? 0 : 1
  when "unpack"
    target = ""
    verbose = false
    i = 1
    while i < ARGV.size
      arg = ARGV[i]
      if arg == "--verbose"
        verbose = true
      elsif target.empty?
        target = arg
      else
        puts colored("Unknown argument: #{arg}", ERROR_STYLE)
        exit 1
      end
      i += 1
    end
    if target.empty?
      puts colored("Missing target argument", ERROR_STYLE)
      exit 1
    end
    if target == "bytes"
      success = unpack_bytes(verbose)
    else
      puts colored("Unknown unpack target: #{target}", ERROR_STYLE)
      success = false
    end
    exit success ? 0 : 1
  when "version"
    success = version_command
    exit success ? 0 : 1
  when "help"
    success = help_command(true)
    exit success ? 0 : 1
  when "help-ui"
    success = run_help_ui
    exit success ? 0 : 1
  else
    puts colored("Unknown command: #{command}", ERROR_STYLE)
    help_command(false)
    exit 1
  end
end
main
