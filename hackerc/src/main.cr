require "file_utils"
require "yaml"
require "json"
require "http/client"
require "colorize"

VERSION = "0.1.1"
HACKER_DIR = File.expand_path("~/.hackeros/hacker-lang")
BIN_DIR = File.join(HACKER_DIR, "bin")
LIBS_DIR = File.join(HACKER_DIR, "libs")
HISTORY_DIR = File.expand_path("~/.hackeros/history")
HISTORY_FILE = File.join(HISTORY_DIR, "hacker_repl_history")

def ensure_hacker_dir
  FileUtils.mkdir_p(HACKER_DIR)
  FileUtils.mkdir_p(BIN_DIR)
  FileUtils.mkdir_p(LIBS_DIR)
  FileUtils.mkdir_p(HISTORY_DIR)
end

def display_welcome
  indent = "  "
  border_color = :light_gray
  text_color = :white
  inside_width = 44
  top = "╔" + "═" * inside_width + "╗"
  bottom = "╚" + "═" * inside_width + "╝"
  left = "║ "
  line1_plain = "Welcome to Hacker Lang CLI v#{VERSION}"
  line2_plain = "Powered by Crystal - Fast & Efficient"
  padding1 = " " * (inside_width - left.size - line1_plain.size)
  padding2 = " " * (inside_width - left.size - line2_plain.size)
  puts indent + top.colorize(border_color).to_s
  puts indent + left.colorize(border_color).to_s + line1_plain.colorize(text_color).to_s + padding1 + "║".colorize(border_color).to_s
  puts indent + left.colorize(border_color).to_s + line2_plain.colorize(text_color).to_s + padding2 + "║".colorize(border_color).to_s
  puts indent + bottom.colorize(border_color).to_s
  puts "#{"Type ".colorize(:cyan)}#{"'hackerc help'".colorize(:yellow)}#{" for a list of commands or ".colorize(:cyan)}#{"'hackerc repl'".colorize(:yellow)}#{" for interactive mode.".colorize(:cyan)}"
  help_command(show_banner: false)
  puts "System ready for commands.".colorize(:light_green)
end

def parse_lines(lines : Array(String), verbose : Bool = false)
  deps = [] of String
  libs = [] of String
  vars = {} of String => String
  cmds = [] of String
  includes = [] of String
  binaries = [] of String
  plugins = [] of String
  errors = [] of String
  config = {} of String => String
  in_config = false
  lines.each_with_index do |line, line_num|
    line = line.strip
    next if line.empty? || line.starts_with?("!")
    if line == "["
      if in_config
        errors << "Line #{line_num + 1}: Nested config block detected"
      end
      in_config = true
      next
    end
    if line == "]"
      if !in_config
        errors << "Line #{line_num + 1}: Unmatched closing bracket"
      end
      in_config = false
      next
    end
    if in_config
      if line.includes?("=")
        k, v = line.split("=", 2)
        config[k.strip] = v.strip
      else
        errors << "Line #{line_num + 1}: Invalid configuration entry: #{line}"
      end
      next
    end
    if line.starts_with?("//")
      deps += line[2..].strip.split
    elsif line.starts_with?("#")
      lib_name = line[1..].strip
      if !lib_name.empty?
        lib_path = File.join(LIBS_DIR, lib_name, "main.hacker")
        if File.exists?(lib_path)
          includes << lib_name
        else
          libs << lib_name
        end
      end
    elsif line.starts_with?("@")
      var_def = line[1..].strip
      if var_def.includes?("=")
        k, v = var_def.split("=", 2)
        vars[k.strip] = v.strip
      else
        errors << "Line #{line_num + 1}: Invalid variable definition: #{line}"
      end
    elsif line.starts_with?("=")
      parts = line[1..].strip.split(">", 2)
      if parts.size == 2
        begin
          n = parts[0].strip.to_i
          cmd = parts[1].strip
          loop_cmd = "for i in $(seq 1 #{n}); do #{cmd}; done"
          cmds << loop_cmd
        rescue
          errors << "Line #{line_num + 1}: Invalid loop count in: #{line}"
        end
      else
        errors << "Line #{line_num + 1}: Invalid loop syntax: #{line}"
      end
    elsif line.starts_with?("?")
      parts = line[1..].strip.split(">", 2)
      if parts.size == 2
        cond = parts[0].strip
        cmd = parts[1].strip
        if_cmd = "if #{cond}; then #{cmd}; fi"
        cmds << if_cmd
      else
        errors << "Line #{line_num + 1}: Invalid conditional syntax: #{line}"
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
    deps: deps.uniq,
    libs: libs,
    vars: vars,
    cmds: cmds,
    includes: includes,
    binaries: binaries,
    plugins: plugins,
    errors: errors,
    config: config
  }
end

def run_command(file : String, verbose : Bool) : Bool
  begin
    lines = File.read_lines(file)
    parsed = parse_lines(lines, verbose)
    if !parsed[:errors].empty?
      puts "Syntax Errors:".colorize(:red)
      parsed[:errors].each { |e| puts e }
      return false
    end
    if !parsed[:libs].empty?
      puts "Warning: Missing custom libraries: #{parsed[:libs].join(", ")}".colorize(:yellow)
      puts "Install them using: bytes install <lib>".colorize(:yellow)
    end
    temp_sh = File.tempfile("hacker_script", ".sh")
    begin
      temp_sh.puts "#!/bin/bash"
      temp_sh.puts "set -e"
      temp_sh.puts "set -u"
      parsed[:vars].each { |k, v| temp_sh.puts "export #{k}=\"#{v}\"" }
      parsed[:deps].each do |dep|
        if dep != "sudo"
          temp_sh.puts "command -v #{dep} &> /dev/null || (sudo apt update && sudo apt install -y #{dep})"
        end
      end
      parsed[:includes].each do |inc|
        lib_path = File.join(LIBS_DIR, inc, "main.hacker")
        temp_sh.puts "# Included from library: #{inc}"
        temp_sh.puts File.read(lib_path)
      end
      parsed[:cmds].each { |cmd| temp_sh.puts cmd }
      parsed[:binaries].each { |bin| temp_sh.puts bin }
      parsed[:plugins].each { |plugin| temp_sh.puts "#{plugin} &" }
      temp_sh.close
      File.chmod(temp_sh.path, 0o755)
      puts "Executing script file: #{file}".colorize(:green)
      puts "Configuration: #{parsed[:config]}".colorize(:cyan)
      puts "Starting execution...".colorize(:light_blue)
      env = ENV.to_h.merge(parsed[:vars])
      result = Process.new("bash", args: [temp_sh.path], env: env, input: Process::Redirect::Inherit, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit).wait.success?
      if !result
        puts "Execution encountered an error".colorize(:red)
        return false
      end
    ensure
      temp_sh.delete
    end
    puts "Execution completed successfully".colorize(:green)
    true
  rescue ex
    puts "Error during execution: #{ex.message}".colorize(:red)
    false
  end
end

def compile_command(file : String, output : String, verbose : Bool) : Bool
  puts "Compiling file #{file} to output #{output} (bash executable)".colorize(:magenta)
  begin
    lines = File.read_lines(file)
    parsed = parse_lines(lines, verbose)
    if !parsed[:errors].empty?
      puts "Syntax Errors:".colorize(:red)
      parsed[:errors].each { |e| puts e }
      return false
    end
    File.open(output, "w") do |out_sh|
      out_sh.puts "#!/bin/bash"
      out_sh.puts "set -e"
      out_sh.puts "set -u"
      parsed[:vars].each { |k, v| out_sh.puts "export #{k}=\"#{v}\"" }
      parsed[:deps].each do |dep|
        if dep != "sudo"
          out_sh.puts "command -v #{dep} &> /dev/null || (sudo apt update && sudo apt install -y #{dep})"
        end
      end
      parsed[:includes].each do |inc|
        lib_path = File.join(LIBS_DIR, inc, "main.hacker")
        out_sh.puts "# Included from library: #{inc}"
        out_sh.puts File.read(lib_path)
      end
      parsed[:cmds].each { |cmd| out_sh.puts cmd }
      parsed[:binaries].each { |bin| out_sh.puts bin }
      parsed[:plugins].each { |plugin| out_sh.puts "#{plugin} &" }
    end
    File.chmod(output, 0o755)
    puts "Compilation process completed successfully".colorize(:green)
    true
  rescue ex
    puts "Compilation error: #{ex.message}".colorize(:red)
    false
  end
end

def check_command(file : String, verbose : Bool) : Bool
  begin
    lines = File.read_lines(file)
    parsed = parse_lines(lines, verbose)
    if !parsed[:errors].empty?
      puts "Syntax Errors:".colorize(:red)
      parsed[:errors].each { |e| puts e }
      return false
    end
    puts "Syntax check passed without issues".colorize(:green)
    true
  rescue ex
    puts "Error during check: #{ex.message}".colorize(:red)
    false
  end
end

def init_command(file : String, verbose : Bool) : Bool
  if File.exists?(file)
    puts "File #{file} already exists".colorize(:yellow)
    return false
  end
  template = [
    "! Hacker Lang advanced template",
    "// sudo ! Privileged operations",
    "// curl ! For downloads",
    "# network-utils ! Custom library example",
    "@APP_NAME=HackerApp ! Application name",
    "@LOG_LEVEL=debug",
    "=3 > echo \"Iteration: $APP_NAME\" ! Loop example",
    "? [ -f /etc/os-release ] > cat /etc/os-release | grep PRETTY_NAME ! Conditional",
    "& ping -c 1 google.com ! Background task",
    "# logging ! Include logging library",
    "> echo \"Starting update...\"",
    "> sudo apt update && sudo apt upgrade -y ! System update",
    "% some_binary_command ! Example binary inclusion",
    "[",
    "Author=Advanced User",
    "Version=1.0",
    "Description=System maintenance script",
    "]",
  ].join("\n")
  begin
    File.write(file, template)
    puts "Template initialized at #{file}".colorize(:green)
    if verbose
      puts "Template Content:".colorize(:cyan)
      puts template.colorize(:light_gray)
    end
    true
  rescue ex
    puts "Initialization error: #{ex.message}".colorize(:red)
    false
  end
end

def clean_command(verbose : Bool) : Bool
  temp_dir = ENV["TMPDIR"]? || "/tmp"
  count = 0
  Dir.glob(File.join(temp_dir, "*.sh")) do |f|
    if File.basename(f).starts_with?("tmp")
      File.delete(f)
      count += 1
      puts "Removed temporary file: #{f}".colorize(:yellow) if verbose
    end
  end
  puts "Cleaned #{count} temporary files".colorize(:green)
  true
end

def unpack_bytes(verbose : Bool) : Bool
  bytes_path1 = File.join(HACKER_DIR, "bin", "bytes")
  bytes_path2 = "/usr/bin/bytes"
  if File.exists?(bytes_path1) || File.exists?(bytes_path2)
    puts "Bytes tool already installed".colorize(:yellow)
    return true
  end
  url = "https://github.com/Bytes-Repository/Bytes-CLI-Tool/releases/download/v0.4/bytes"
  begin
    HTTP::Client.get(url) do |response|
      if response.success?
        FileUtils.mkdir_p(File.dirname(bytes_path1))
        File.open(bytes_path1, "wb") do |f|
          IO.copy(response.body_io, f)
        end
        File.chmod(bytes_path1, 0o755)
        puts "Downloaded and installed bytes from #{url} to #{bytes_path1}".colorize(:cyan) if verbose
        puts "Bytes installation completed successfully".colorize(:green)
        return true
      else
        puts "Download error: status code #{response.status_code}".colorize(:red)
        return false
      end
    end
  rescue ex
    puts "Error installing bytes: #{ex.message}".colorize(:red)
    false
  end
end

def editor_command(file : String?) : Bool
  editor_path = File.join(HACKER_DIR, "bin", "hacker-editor.AppImage")
  if !File.exists?(editor_path)
    puts "Editor not found at #{editor_path}. Please ensure it is installed.".colorize(:red)
    return false
  end
  args = file ? [file] : [] of String
  puts "Launching editor with arguments: #{args.join(" ")}".colorize(:magenta)
  begin
    Process.run(editor_path, args: args, input: Process::Redirect::Inherit, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
    puts "Editor session has completed".colorize(:green)
    true
  rescue ex
    puts "Editor launch failed: #{ex.message}".colorize(:red)
    false
  end
end

def run_repl(verbose : Bool) : Bool
  repl_path = File.join(BIN_DIR, "hackerc", "repl")
  args = verbose ? ["--verbose"] : [] of String
  puts "Starting REPL session...".colorize(:light_blue)
  begin
    Process.run(repl_path, args: args, input: Process::Redirect::Inherit, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
    puts "REPL session has ended".colorize(:green)
    true
  rescue ex
    puts "REPL error: #{ex.message}".colorize(:red)
    false
  end
end

def run_help_ui : Bool
  help_ui_path = File.join(BIN_DIR, "hackerc", "help-ui")
  puts "Launching Help UI interface...".colorize(:magenta)
  begin
    Process.run(help_ui_path, input: Process::Redirect::Inherit, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
    true
  rescue ex
    puts "Help UI error: #{ex.message}".colorize(:red)
    false
  end
end

def version_command : Bool
  puts "Hacker Lang CLI version #{VERSION}".colorize(:cyan)
  puts "Enhanced edition with expanded features".colorize(:light_green)
  true
end

def help_command(show_banner : Bool = true) : Bool
  if show_banner
    puts "Hacker Lang CLI - Advanced Scripting Tool".colorize(:green).bold
  end
  puts "Commands Overview:".colorize(:cyan).bold
  column1_width = 15
  column2_width = 50
  column3_width = 50
  format = "│ %-#{column1_width}s │ %-#{column2_width}s │ %-#{column3_width}s │"
  top_border = "┌" + "─" * (column1_width + 2) + "┬" + "─" * (column2_width + 2) + "┬" + "─" * (column3_width + 2) + "┐"
  middle_border = "├" + "─" * (column1_width + 2) + "┼" + "─" * (column2_width + 2) + "┼" + "─" * (column3_width + 2) + "┤"
  bottom_border = "└" + "─" * (column1_width + 2) + "┴" + "─" * (column2_width + 2) + "┴" + "─" * (column3_width + 2) + "┘"
  puts top_border.colorize(:light_gray)
  puts (format % ["Command", "Description", "Arguments"]).colorize(:light_gray)
  puts middle_border.colorize(:light_gray)
  commands = [
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
  commands.each do |command|
    cmd, desc, args = command
    puts (format % [cmd, desc, args]).colorize(:white)
  end
  puts bottom_border.colorize(:light_gray)
  true
end

def run_bytes_project(verbose : Bool) : Bool
  bytes_file = "hacker.bytes"
  begin
    content = File.read(bytes_file)
    project = YAML.parse(content)
    puts "Running project #{project["package"]["name"]} version #{project["package"]["version"]} by #{project["package"]["author"]}".colorize(:magenta)
    run_command(project["entry"].as_s, verbose)
  rescue ex
    puts "Project error: #{ex.message}".colorize(:red)
    false
  end
end

def compile_bytes_project(output : String, verbose : Bool) : Bool
  bytes_file = "hacker.bytes"
  begin
    content = File.read(bytes_file)
    project = YAML.parse(content)
    output = project["package"]["name"].as_s if output.empty?
    compile_command(project["entry"].as_s, output, verbose)
  rescue ex
    puts "Project compilation error: #{ex.message}".colorize(:red)
    false
  end
end

def main
  ensure_hacker_dir
  if ARGV.empty?
    display_welcome
    return
  end
  command = ARGV[0]
  args = ARGV.size > 1 ? ARGV[1..] : [] of String
  verbose = args.includes?("--verbose")
  case command
  when "run"
    file = args.find { |arg| !arg.starts_with?("--") } || "."
    success = file == "." ? run_bytes_project(verbose) : run_command(file, verbose)
    exit success ? 0 : 1
  when "compile"
    file = args.find { |arg| !arg.starts_with?("--") && arg != "-o" }
    output_index = args.index("-o")
    output = if output_index && output_index + 1 < args.size
               args[output_index + 1]
             else
               file ? File.basename(file.to_s, ".*") : ""
             end
    bytes_mode = args.includes?("--bytes")
    success = bytes_mode ? compile_bytes_project(output.to_s, verbose) : compile_command(file.to_s, output.to_s, verbose)
    exit success ? 0 : 1
  when "check"
    file = args.find { |arg| !arg.starts_with?("--") }
    success = check_command(file.to_s, verbose)
    exit success ? 0 : 1
  when "init"
    file = args.find { |arg| !arg.starts_with?("--") }
    success = init_command(file.to_s, verbose)
    exit success ? 0 : 1
  when "clean"
    success = clean_command(verbose)
    exit success ? 0 : 1
  when "repl"
    success = run_repl(verbose)
    exit success ? 0 : 1
  when "editor"
    file = args.find { |arg| !arg.starts_with?("--") }
    success = editor_command(file)
    exit success ? 0 : 1
  when "unpack"
    target = args.find { |arg| !arg.starts_with?("--") }
    if target == "bytes"
      success = unpack_bytes(verbose)
    else
      puts "Unknown unpack target: #{target}".colorize(:red)
      success = false
    end
    exit success ? 0 : 1
  when "version"
    success = version_command
    exit success ? 0 : 1
  when "help"
    success = help_command
    exit success ? 0 : 1
  when "help-ui"
    success = run_help_ui
    exit success ? 0 : 1
  else
    puts "Unknown command: #{command}".colorize(:red)
    help_command
    exit 1
  end
end

main
