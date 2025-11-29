# Bytes.io CLI Tool

![Bytes.io Logo](https://example.com/logo.png) <!-- Replace with actual logo if available -->

## Overview

Bytes.io CLI is a command-line tool for managing libraries and plugins for Hacker Lang (part of HackerOS). It allows you to search, install, remove, and update packages from the Bytes.io repository. The tool is built in Crystal and provides a simple interface for developers working with Hacker Lang.

- **Version**: 0.5
- **Repository**: [https://github.com/Bytes-Repository/bytes.io](https://github.com/Bytes-Repository/bytes.io)
- **Main Repo**: [https://github.com/Bytes-Repository/Bytes-CLI-Tool/](https://github.com/Bytes-Repository/Bytes-CLI-Tool/)
- **Libs Installation Path**: `~/.hackeros/hacker-lang/libs/`
- **Plugins Installation Path**: `~/.hackeros/hacker-lang/plugins/`

## Features

- Search for available packages and plugins.
- Install and remove libraries/plugins.
- Update all installed packages.
- Refresh the local repository cache.
- Support for community-contributed packages.
- Colorized output for better readability.
- Progress bar for downloads.

## Installation

To install the Bytes.io CLI tool, you need Crystal installed on your system. Follow these steps:

1. Install Crystal (if not already installed):
   - On Debian/Ubuntu: `sudo apt install crystal`
   - On macOS: `brew install crystal`
   - For other platforms, see [Crystal installation guide](https://crystal-lang.org/install/).

2. Clone the repository or download the source files (`main.cr`, `repo.cr`, `download.cr`, `commands.cr`).

3. Compile the tool:
   ```
   crystal build main.cr -o bytes
   ```

4. Move the binary to a directory in your PATH (e.g., `/usr/local/bin/`):
   ```
   sudo mv bytes /usr/local/bin/
   ```

5. Run the tool:
   ```
   bytes --help
   ```

Note: The tool creates necessary directories in `~/.hackeros/hacker-lang/` automatically.

## Usage

Run the tool with commands like `bytes <command> [args]`. If no command is provided, it shows the usage help.

### General Commands

- `search <query>`: Search for packages matching the query.
  - Example: `bytes search obsidian`

- `install <package>`: Install a package from the repository.
  - Example: `bytes install obsidian-lib`

- `remove <package>`: Remove an installed package.
  - Example: `bytes remove obsidian-lib`

- `update`: Update all installed libraries to the latest versions.
  - Example: `bytes update`

- `refresh`: Refresh the local repository cache.
  - Example: `bytes refresh`

- `info`: Show information about the tool.
  - Example: `bytes info`

- `how-to-use`: Show guide on how to use the tool and add custom repos.
  - Example: `bytes how-to-use`

- `how-to-add`: Show how to add your own repository.
  - Example: `bytes how-to-add`

### Plugin Commands

Plugins are managed under the `plugin` subcommand.

- `plugin search <query>`: Search for plugins matching the query.
  - Example: `bytes plugin search myplugin`

- `plugin install <plugin>`: Install a plugin.
  - Example: `bytes plugin install myplugin`

- `plugin remove <plugin>`: Remove an installed plugin.
  - Example: `bytes plugin remove myplugin`

- `plugin update`: Update all installed plugins.
  - Example: `bytes plugin update`

- `plugin refresh`: Refresh the plugin repository cache.
  - Example: `bytes plugin refresh`

## Repository Structure

The main repository is fetched from: [https://raw.githubusercontent.com/Bytes-Repository/bytes.io/main/repository/bytes.io](https://raw.githubusercontent.com/Bytes-Repository/bytes.io/main/repository/bytes.io)

The plugin repository is fetched from: [https://raw.githubusercontent.com/Bytes-Repository/bytes.io/main/repository/plugins-repo.hacker](https://raw.githubusercontent.com/Bytes-Repository/bytes.io/main/repository/plugins-repo.hacker)

Packages are organized in sections (e.g., Official, Community) and categories.

## How to Add Your Own Package

To contribute your library or plugin:

1. Fork the [bytes.io repository](https://github.com/Bytes-Repository/bytes.io) on GitHub.

2. Edit the `repository/bytes.io` file (for libs) or `repository/plugins-repo.hacker` (for plugins) in the Community section.

3. Format example for libs:
   ```
   Community:
     CATEGORY:
       your-lib: https://your-release-url
   ```

4. Create a pull request to the main repository.

5. Once merged, your package will be available via the CLI.

Alternatively, report your repository via [issues](https://github.com/Bytes-Repository/bytes.io/issues) or [discussions](https://github.com/Bytes-Repository/bytes.io/discussions).

## Troubleshooting

- **Error downloading: Bad status: 618 for HEAD**: This was fixed in the updated code by handling redirects properly in the download process.
- If directories can't be created, check permissions in your home directory.
- For network issues, ensure you have internet access and no proxy blocks GitHub.

## Development

- The tool uses Crystal's `HTTP::Client` for downloads and YAML for parsing repos.
- Dependencies: `option_parser`, `colorize`, `file_utils`.
- To build and test: `crystal build main.cr && ./main install obsidian-lib`

## License

This tool is open-source under the MIT License. See [LICENSE](LICENSE) for details.

## Credits

Created by the HackerOS Team. Contributions welcome!
