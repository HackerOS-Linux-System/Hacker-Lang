require "file_utils"
require "option_parser"
require "path"

module Hl
  VERSION = "1.2"
  HACKER_DIR = "~/.hackeros/hacker-lang"
  BIN_DIR = "bin"
  COMPILER_PATH = "hacker-compiler"
  RUNTIME_PATH = "hacker-runtime"

  def self.ensure_hacker_dir
    home = ENV["HOME"]? || ""
    full_bin_dir = Path.new(home, HACKER_DIR, BIN_DIR).expand.to_s
    Dir.mkdir_p(full_bin_dir)
  rescue error
    puts "Failed to create hacker directory: #{error.message}"
    exit(1)
  end

  def self.display_welcome
    puts "Welcome to Hacker Lang CLI v#{VERSION}"
    puts "Simplified tool for running and compiling .hacker scripts"
    puts "Type 'hl help' for available commands."
    help_command(false)
  end

  def self.run_command(file : String, verbose : Bool) : Bool
    home = ENV["HOME"]? || ""
    full_runtime_path = Path.new(home, HACKER_DIR, BIN_DIR, RUNTIME_PATH).expand.to_s
    unless File.exists?(full_runtime_path)
      puts "Hacker runtime not found at #{full_runtime_path}. Please install the Hacker Lang tools."
      return false
    end
    args = [file]
    args << "--verbose" if verbose
    puts "Executing script: #{file}#{verbose ? " (verbose mode)" : ""}"
    begin
      process = Process.new(full_runtime_path, args, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
      status = process.wait
      if status.success?
        puts "Execution completed successfully."
        return true
      else
        puts "Execution failed with exit code #{status.exit_code}"
        return false
      end
    rescue error
      puts "Error executing command: #{error.message}"
      return false
    end
  end

  def self.compile_command(file : String, output : String, verbose : Bool) : Bool
    home = ENV["HOME"]? || ""
    full_compiler_path = Path.new(home, HACKER_DIR, BIN_DIR, COMPILER_PATH).expand.to_s
    unless File.exists?(full_compiler_path)
      puts "Hacker compiler not found at #{full_compiler_path}. Please install the Hacker Lang tools."
      return false
    end
    args = [file, output]
    args << "--verbose" if verbose
    puts "Compiling script: #{file} to #{output}#{verbose ? " (verbose mode)" : ""}"
    begin
      process = Process.new(full_compiler_path, args, output: Process::Redirect::Inherit, error: Process::Redirect::Inherit)
      status = process.wait
      if status.success?
        puts "Compilation completed successfully."
        return true
      else
        puts "Compilation failed with exit code #{status.exit_code}"
        return false
      end
    rescue error
      puts "Error executing command: #{error.message}"
      return false
    end
  end

  def self.help_command(show_banner : Bool)
    if show_banner
      puts "Hacker Lang CLI - Simplified Scripting Tool v#{VERSION}"
    end
    puts "Available Commands:"
    puts "Command | Description | Usage"
    puts "------------|----------------------------------|----------------------------------"
    puts "run | Execute a .hacker script | hl run <file> [--verbose]"
    puts "compile | Compile to native executable | hl compile <file> [-o output] [--verbose]"
    puts "help | Show this help menu | hl help"
    puts "\nGlobal options:"
    puts "-v, --version Display version"
    puts "-h, --help Display help"
    true
  end

  def self.version_command
    puts "Hacker Lang CLI v#{VERSION}"
    true
  end

  def self.main
    ensure_hacker_dir
    if ARGV.empty?
      display_welcome
      exit(0)
    end
    # Global flags
    show_version = false
    show_help = false
    OptionParser.parse do |parser|
      parser.on("-v", "--version", "Display version") { show_version = true }
      parser.on("-h", "--help", "Display help") { show_help = true }
    end
    if show_version
      version_command
      exit(0)
    end
    if show_help
      help_command(true)
      exit(0)
    end
    if ARGV.empty?
      display_welcome
      exit(0)
    end
    command = ARGV.shift
    success = true
    case command
    when "run"
      verbose = false
      file = ""
      OptionParser.parse do |parser|
        parser.banner = "Usage: hl run <file> [options]"
        parser.on("--verbose", "Enable verbose output") { verbose = true }
        parser.unknown_args do |args|
          if args.size != 1
            puts "Error: Expected exactly one argument: <file>"
            puts parser
            exit(1)
          end
          file = args[0]
        end
      end
      success = run_command(file, verbose)
    when "compile"
      verbose = false
      output = ""
      file = ""
      OptionParser.parse do |parser|
        parser.banner = "Usage: hl compile <file> [options]"
        parser.on("-o OUTPUT", "--output=OUTPUT", "Specify output file") { |o| output = o }
        parser.on("--verbose", "Enable verbose output") { verbose = true }
        parser.unknown_args do |args|
          if args.size != 1
            puts "Error: Expected exactly one argument: <file>"
            puts parser
            exit(1)
          end
          file = args[0]
        end
      end
      if output.empty?
        output = File.basename(file, File.extname(file))
      end
      success = compile_command(file, output, verbose)
    when "help"
      success = help_command(true)
    else
      puts "Unknown command: #{command}"
      help_command(false)
      success = false
    end
    if success
      exit(0)
    else
      exit(1)
    end
  end
end

Hl.main
