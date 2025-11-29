require "option_parser"
require "colorize"
require "file_utils"
require "./repo"
require "./commands"
require "./download"
APP_NAME = "Bytes.io CLI Tool"
VERSION = "0.5"
REPO_URL = "https://raw.githubusercontent.com/Bytes-Repository/bytes.io/main/repository/bytes.io"
LOCAL_REPO_PATH = "/tmp/bytes.io"
PLUGIN_REPO_URL = "https://raw.githubusercontent.com/Bytes-Repository/bytes.io/main/repository/plugins-repo.hacker"
LOCAL_PLUGIN_REPO = "/tmp/plugins-repo.hacker"
LIB_DIR_SUFFIX = "/.hackeros/hacker-lang/libs/"
PLUGIN_DIR_SUFFIX = "/.hackeros/hacker-lang/plugins/"
# Styles
def bold_style(text : String) : String
  text.colorize.bold.fore(:white).to_s
end
def success_style(text : String) : String
  text.colorize.fore(:green).to_s
end
def error_style(text : String) : String
  text.colorize.fore(:red).to_s
end
def info_style(text : String) : String
  text.colorize.fore(:cyan).to_s
end
def warn_style(text : String) : String
  text.colorize.fore(:yellow).to_s
end
def center_text(text : String, width : Int32) : String
  len = text.size
  if len >= width
    text
  else
    left = (width - len) // 2
    right = width - len - left
    " " * left + text + " " * right
  end
end
def header_style(text : String, width : Int32 = 60) : String
  padded = center_text(text, width).colorize.bold.fore(:yellow).to_s
  border = ("─" * width).colorize.fore(:magenta).to_s
  "#{padded}\n#{border}"
end
def footer_style(text : String) : String
  text.colorize.italic.fore(:dark_gray).to_s
end
def main
  if ARGV.empty?
    print_usage
    exit(1)
  end
  home = ENV["HOME"]? || begin
    puts error_style("Error getting user home")
    exit(1)
  end
  lib_dir = File.join(home, LIB_DIR_SUFFIX)
  Dir.mkdir_p(lib_dir) rescue begin
    puts error_style("Error creating lib dir")
    exit(1)
  end
  plugin_dir = File.join(home, PLUGIN_DIR_SUFFIX)
  Dir.mkdir_p(plugin_dir) rescue begin
    puts error_style("Error creating plugin dir")
    exit(1)
  end
  cmd = ARGV[0]
  case cmd
  when "plugin"
    if ARGV.size < 2
      puts error_style("Usage: plugin <subcommand> [args]")
      print_plugin_usage
      exit(1)
    end
    subcmd = ARGV[1]
    unless File.exists?(LOCAL_PLUGIN_REPO)
      begin
        refresh_repo(PLUGIN_REPO_URL, LOCAL_PLUGIN_REPO)
      rescue ex
        puts error_style("Error refreshing plugin repo: #{ex.message}")
        exit(1)
      end
    end
    repo = begin
      parse_repo(LOCAL_PLUGIN_REPO)
    rescue ex
      puts error_style("Error parsing plugin repo: #{ex.message}")
      exit(1)
    end
    case subcmd
    when "search"
      if ARGV.size < 3
        puts error_style("Usage: plugin search <query>")
        exit(1)
      end
      query = ARGV[2]
      search_packages(repo, query)
    when "install"
      if ARGV.size < 3
        puts error_style("Usage: plugin install <plugin>")
        exit(1)
      end
      pkg = ARGV[2]
      install_package(repo, pkg, plugin_dir)
    when "remove"
      if ARGV.size < 3
        puts error_style("Usage: plugin remove <plugin>")
        exit(1)
      end
      pkg = ARGV[2]
      remove_package(pkg, plugin_dir)
    when "update"
      update_packages(plugin_dir, LOCAL_PLUGIN_REPO)
    when "refresh"
      begin
        refresh_repo(PLUGIN_REPO_URL, LOCAL_PLUGIN_REPO)
      rescue ex
        puts error_style("Error refreshing: #{ex.message}")
      end
      puts success_style("Plugin repo refreshed successfully.")
    else
      puts error_style("Unknown plugin subcommand: #{subcmd}")
      print_plugin_usage
      exit(1)
    end
  when "search"
    if ARGV.size < 2
      puts error_style("Usage: search <query>")
      exit(1)
    end
    unless File.exists?(LOCAL_REPO_PATH)
      begin
        refresh_repo(REPO_URL, LOCAL_REPO_PATH)
      rescue ex
        puts error_style("Error refreshing repo: #{ex.message}")
        exit(1)
      end
    end
    query = ARGV[1]
    repo = begin
      parse_repo(LOCAL_REPO_PATH)
    rescue ex
      puts error_style("Error parsing repo: #{ex.message}")
      exit(1)
    end
    search_packages(repo, query)
  when "install"
    if ARGV.size < 2
      puts error_style("Usage: install <package>")
      exit(1)
    end
    unless File.exists?(LOCAL_REPO_PATH)
      begin
        refresh_repo(REPO_URL, LOCAL_REPO_PATH)
      rescue ex
        puts error_style("Error refreshing repo: #{ex.message}")
        exit(1)
      end
    end
    pkg = ARGV[1]
    repo = begin
      parse_repo(LOCAL_REPO_PATH)
    rescue ex
      puts error_style("Error parsing repo: #{ex.message}")
      exit(1)
    end
    install_package(repo, pkg, lib_dir)
  when "remove"
    if ARGV.size < 2
      puts error_style("Usage: remove <package>")
      exit(1)
    end
    pkg = ARGV[1]
    remove_package(pkg, lib_dir)
  when "update"
    unless File.exists?(LOCAL_REPO_PATH)
      begin
        refresh_repo(REPO_URL, LOCAL_REPO_PATH)
      rescue ex
        puts error_style("Error refreshing repo: #{ex.message}")
        exit(1)
      end
    end
    update_packages(lib_dir, LOCAL_REPO_PATH)
  when "refresh"
    begin
      refresh_repo(REPO_URL, LOCAL_REPO_PATH)
    rescue ex
      puts error_style("Error refreshing: #{ex.message}")
    end
    puts success_style("Repo refreshed successfully.")
  when "info"
    print_info
  when "how-to-use"
    print_how_to_use
  when "how-to-add"
    print_how_to_add
  else
    print_usage
    exit(1)
  end
end
def print_usage
  header = header_style("#{APP_NAME} v#{VERSION}")
  commands = %(
Commands:
search <query> - Search for packages
install <package> - Install a package
remove <package> - Remove a package
update - Update all installed libraries
refresh - Refresh the repository
info - Show tool information
how-to-use - Show how to use and add custom repos
how-to-add - Show how to add your repository
Plugin Commands:
plugin search <query> - Search for plugins
plugin install <plugin> - Install a plugin
plugin remove <plugin> - Remove a plugin
plugin update - Update all installed plugins
plugin refresh - Refresh the plugin repository
  )
  footer = footer_style("Created by HackerOS Team")
  puts "#{header}\n#{info_style(commands.strip)}\n#{footer}"
end
def print_plugin_usage
  commands = %(
Plugin Commands:
search <query> - Search for plugins
install <plugin> - Install a plugin
remove <plugin> - Remove a plugin
update - Update all installed plugins
refresh - Refresh the plugin repository
  )
  puts info_style(commands.strip)
end
def print_info
  info = %(
Bytes.io CLI Tool for Hacker Lang (HackerOS)
Version: #{VERSION}
Repository: https://github.com/Bytes-Repository/bytes.io
Libs installed in: ~/.hackeros/hacker-lang/libs/
Plugins installed in: ~/.hackeros/hacker-lang/plugins/
  )
  puts info_style(info.strip)
end
def print_how_to_use
  guide = %(
How to use and add your own repo to bytes.io:
1. Fork the bytes.io repository on GitHub.
2. Add your library to the repository/bytes.io file in the Community section.
3. Format: Community: { CATEGORY: { your-lib: https://your-release-url } }
4. Create a pull request to the main repo.
5. Once merged, your lib will be available via this tool.
  )
  puts info_style(guide.strip)
  puts success_style("Happy hacking!")
end
def print_how_to_add
  guide = %(
How to add your repository:
Zgłoś swoje repozytorium w https://github.com/Bytes-Repository/bytes.io/issues lub https://github.com/Bytes-Repository/bytes.io/discussions
Alternatively, follow the how-to-use guide to submit via PR.
  )
  puts info_style(guide.strip)
end
main

