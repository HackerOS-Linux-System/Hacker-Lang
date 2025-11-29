require "colorize"
require "option_parser"
require "process"

VERSION = "1.2"
HACKER_DIR = File.expand_path("~/.hackeros/hacker-lang")
BIN_DIR = File.join(HACKER_DIR, "bin")
COMPILER_PATH = File.join(BIN_DIR, "hacker-compiler")
RUNTIME_PATH = File.join(BIN_DIR, "hacker-runtime")

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
end

def display_welcome
  puts "#{COLOR_BOLD}#{COLOR_PURPLE}Welcome to Hacker Lang CLI v#{VERSION}#{COLOR_RESET}"
  puts "#{COLOR_GRAY}Simplified tool for running and compiling .hacker scripts#{COLOR_RESET}"
  puts "#{COLOR_WHITE}Type 'hackerc help' for available commands.#{COLOR_RESET}\n"
  help_command(true)
end

def run_command(file : String, verbose : Bool) : Bool
  if !File.exists?(RUNTIME_PATH)
    puts "#{COLOR_RED}Hacker runtime not found at #{RUNTIME_PATH}. Please install the Hacker Lang tools.#{COLOR_RESET}"
    return false
  end
  args = [file]
  args << "--verbose" if verbose
  puts "#{COLOR_CYAN}Executing script: #{file}#{verbose ? " (verbose mode)" : ""}#{COLOR_RESET}"
  status = Process.run(RUNTIME_PATH, args, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
  if status.success?
    puts "#{COLOR_GREEN}Execution completed successfully.#{COLOR_RESET}"
  else
    puts "#{COLOR_RED}Execution failed with exit code #{status.exit_code}.#{COLOR_RESET}"
  end
  status.success?
end

def compile_command(file : String, output : String, verbose : Bool) : Bool
  if !File.exists?(COMPILER_PATH)
    puts "#{COLOR_RED}Hacker compiler not found at #{COMPILER_PATH}. Please install the Hacker Lang tools.#{COLOR_RESET}"
    return false
  end
  args = [file, output]
  args << "--verbose" if verbose
  puts "#{COLOR_CYAN}Compiling script: #{file} to #{output}#{verbose ? " (verbose mode)" : ""}#{COLOR_RESET}"
  status = Process.run(COMPILER_PATH, args, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
  if status.success?
    puts "#{COLOR_GREEN}Compilation completed successfully.#{COLOR_RESET}"
  else
    puts "#{COLOR_RED}Compilation failed with exit code #{status.exit_code}.#{COLOR_RESET}"
  end
  status.success?
end

def help_command(show_banner : Bool) : Bool
  if show_banner
    puts "#{COLOR_BOLD}#{COLOR_PURPLE}Hacker Lang CLI - Simplified Scripting Tool v#{VERSION}#{COLOR_RESET}\n"
  end
  puts "#{COLOR_BOLD}Available Commands:#{COLOR_RESET}"
  puts "#{"Command".ljust(15).colorize(:light_gray)} #{"Description".ljust(40).colorize(:light_gray)} #{"Usage".ljust(40).colorize(:light_gray)}"
  commands = [
    ["run", "Execute a .hacker script", "hackerc run <file> [--verbose]"],
    ["compile", "Compile to native executable", "hackerc compile <file> [-o output] [--verbose]"],
    ["help", "Show this help menu", "hackerc help"],
  ]
  commands.each do |cmd|
    puts "#{cmd[0].ljust(15).colorize(:cyan)} #{cmd[1].ljust(40)} #{cmd[2].ljust(40).colorize(:yellow)}"
  end
  puts "\n#{COLOR_GRAY}Global options:#{COLOR_RESET}"
  puts "-v, --version    Display version"
  puts "-h, --help       Display help"
  true
end

def version_command : Bool
  puts "#{COLOR_CYAN}Hacker Lang CLI v#{VERSION}#{COLOR_RESET}"
  true
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

  case command
  when "run"
    parser = OptionParser.new do |p|
      p.banner = "#{COLOR_BOLD}Usage:#{COLOR_RESET} hackerc run <file> [options]\n\nExecute a .hacker script."
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
      puts "#{COLOR_RED}Error: Expected exactly one argument: <file>#{COLOR_RESET}"
      puts parser
      exit(1)
    end
    file = ARGV.shift
    success = run_command(file.not_nil!, verbose)
  when "compile"
    parser = OptionParser.new do |p|
      p.banner = "#{COLOR_BOLD}Usage:#{COLOR_RESET} hackerc compile <file> [options]\n\nCompile to native executable."
      p.on("-o OUTPUT", "--output OUTPUT", "Specify output file") { |o| output = o }
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
      puts "#{COLOR_RED}Error: Expected exactly one argument: <file>#{COLOR_RESET}"
      puts parser
      exit(1)
    end
    file = ARGV.shift
    output = output.empty? ? File.basename(file.not_nil!, File.extname(file.not_nil!)) : output
    success = compile_command(file.not_nil!, output, verbose)
  when "help"
    success = help_command(true)
  else
    puts "#{COLOR_RED}Unknown command: #{command}#{COLOR_RESET}"
    help_command(false)
    success = false
  end
  exit(success ? 0 : 1)
end

main
