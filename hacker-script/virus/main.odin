package main

import "core:bufio"
import "core:fmt"
import "core:os"
import "core:path/filepath"
import "core:slice"
import "core:strings"
import "core:time"

// For JSON parsing (assuming a simple JSON lib or implement minimally)
import "core:encoding/json"

// For HTTP downloads, use os.exec with curl for simplicity (assuming curl is available)
import "core:sys/unix" // For exec

// Constants
HACKER_SCRIPT_HOME :: "/home/HackerScript"
CORE_LIBS_DIR :: filepath.join({HACKER_SCRIPT_HOME, "core"})
LIBS_DIR :: filepath.join({HACKER_SCRIPT_HOME, "libs"})
VERSION_FILE :: filepath.join({HACKER_SCRIPT_HOME, "version.hacker"})
INDEX_URL :: "https://github.com/HackerOS-Linux-System/Hacker-Lang/blob/main/hacker-script/index.json"
VERSION_REMOTE_URL :: "https://github.com/HackerOS-Linux-System/Hacker-Lang/blob/main/hacker-packages/HackerScript-Version.hacker"
RELEASE_URL_TEMPLATE :: "https://github.com/HackerOS-Linux-System/Hacker-Lang/releases/download/v%s/HackerScript.tar.gz"

// Color codes for beautiful CLI
RED :: "\e[31m"
GREEN :: "\e[32m"
YELLOW :: "\e[33m"
BLUE :: "\e[34m"
RESET :: "\e[0m"

print_colored :: proc(color: string, format: string, args: ..any) {
    fmt.printf("%s", color)
    fmt.printf(format, ..args)
    fmt.printf("%s\n", RESET)
}

// Version struct (simple array as per example [0.1])
Version :: []f64 // Actually semver, but example is [0.1]

// Helper to read file content
read_file :: proc(path: string) -> (string, bool) {
    data, ok := os.read_entire_file(path)
    if !ok {
        return "", false
    }
    return strings.clone_from_bytes(data), true
}

// Helper to download file using curl
download_file :: proc(url: string, dest: string) -> bool {
    args := []string{"curl", "-L", "-o", dest, url}
    pid := unix.fork()
    if pid == 0 {
        unix.execvp("curl", args[:])
    }
    _, status := unix.waitpid(pid)
    return status == 0
}

// Parse version file (JSON array)
parse_version :: proc(content: string) -> (Version, bool) {
    var v: Version
    err := json.unmarshal_string(content, &v)
    return v, err == .None
}

// Compare versions (simple: assume single number, higher is newer)
is_newer :: proc(local, remote: Version) -> bool {
    if len(local) == 0 || len(remote) == 0 {
        return false
    }
    return remote[0] > local[0]
}

// Update command implementation
do_update :: proc() {
    local_content, local_ok := read_file(VERSION_FILE)
    if !local_ok {
        print_colored(RED, "Failed to read local version file")
        return
    }
    local_ver, local_parse_ok := parse_version(local_content)
    if !local_parse_ok {
        print_colored(RED, "Invalid local version format")
        return
    }

    temp_ver_file := "/tmp/remote_version.hacker"
    if !download_file(VERSION_REMOTE_URL, temp_ver_file) {
        print_colored(RED, "Failed to download remote version")
        return
    }
    remote_content, remote_ok := read_file(temp_ver_file)
    if !remote_ok {
        print_colored(RED, "Failed to read remote version")
        return
    }
    remote_ver, remote_parse_ok := parse_version(remote_content)
    if !remote_parse_ok {
        print_colored(RED, "Invalid remote version format")
        return
    }

    if is_newer(local_ver, remote_ver) {
        ver_str := fmt.tprintf("%.1f", remote_ver[0])
        tar_file := fmt.tprintf("/tmp/HackerScript-v%s.tar.gz", ver_str)
        release_url := strings.replace(RELEASE_URL_TEMPLATE, "%s", ver_str, 1)
        if !download_file(release_url, tar_file) {
            print_colored(RED, "Failed to download release tar.gz")
            return
        }

        // Extract (use tar command)
        args := []string{"tar", "-xzf", tar_file, "-C", HACKER_SCRIPT_HOME}
        pid := unix.fork()
        if pid == 0 {
            unix.execvp("tar", args[:])
        }
        _, status := unix.waitpid(pid)
        if status != 0 {
            print_colored(RED, "Failed to extract tar.gz")
            return
        }

        // Set permissions (chmod +x on binaries)
        bins := []string{
            filepath.join({HACKER_SCRIPT_HOME, "bin", "HackerScript-Compiler"}),
            filepath.join({HACKER_SCRIPT_HOME, "bin", "HackerScript-Runtime"}),
        }
        for bin in bins {
            os.chmod(bin, 0o755)
        }

        print_colored(GREEN, "Updated to version %.1f", remote_ver[0])
    } else {
        print_colored(GREEN, "Already up to date")
    }
}

// Build command
do_build :: proc(args: []string) {
    production := false
    for arg in args {
        if arg == "--production" {
            production = true
        }
    }

    print_colored(BLUE, "Building project%s...", production ? " in production mode" : "")

    // Create isolated env
    iso_dir := "/tmp/hs_iso_env"
    os.make_directory(iso_dir)

    // Install deps (stub)
    // For now, assume copying from LIBS_DIR

    // Compile: assume running HackerScript-Compiler on main.hcs
    compiler := filepath.join({HACKER_SCRIPT_HOME, "bin", "HackerScript-Compiler"})
    main_hcs := "cmd/main.hcs" // Relative to cwd
    output_bin := "build/output"

    // Exec compiler (assuming it's Python, use python3)
    cmd_args := []string{"python3", compiler, main_hcs, output_bin}
    pid := unix.fork()
    if pid == 0 {
        unix.execvp("python3", cmd_args[:])
    }
    _, status := unix.waitpid(pid)
    if status != 0 {
        print_colored(RED, "Build failed")
        return
    }

    // Link libs (assuming already handled in compiler)

    // Check errors (stub)

    print_colored(GREEN, "Build successful: %s", output_bin)
}

// Install library
do_install :: proc(lib_name: string) {
    print_colored(BLUE, "Installing library: %s", lib_name)
    // Fetch from index.json
    index_file := "/tmp/hs_index.json"
    if !download_file(INDEX_URL, index_file) {
        print_colored(RED, "Failed to download index")
        return
    }
    // Parse index (assume JSON object with lib: url)
    content, ok := read_file(index_file)
    if !ok {
        print_colored(RED, "Failed to read index")
        return
    }
    var index: map[string]string
    err := json.unmarshal_string(content, &index)
    if err != .None {
        print_colored(RED, "Invalid index format")
        return
    }
    lib_url, found := index[lib_name]
    if !found {
        print_colored(RED, "Library not found in index")
        return
    }
    lib_dest := filepath.join({LIBS_DIR, lib_name})
    if !download_file(lib_url, lib_dest) {
        print_colored(RED, "Failed to download library")
        return
    }
    print_colored(GREEN, "Installed %s", lib_name)
}

// Remove library
do_remove :: proc(lib_name: string) {
    print_colored(BLUE, "Removing library: %s", lib_name)
    lib_path := filepath.join({LIBS_DIR, lib_name})
    if os.remove(lib_path) != 0 {
        print_colored(RED, "Failed to remove library")
        return
    }
    print_colored(GREEN, "Removed %s", lib_name)
}

// Help command
do_help :: proc() {
    print_colored(YELLOW, "Virus CLI - HackerScript Manager")
    fmt.println("Commands:")
    fmt.println("  build [--production]  - Build the project")
    fmt.println("  install <lib>         - Install a library")
    fmt.println("  remove <lib>          - Remove a library")
    fmt.println("  update                - Check and apply updates")
    fmt.println("  docs                  - Show documentation")
    fmt.println("  tutorial              - Show tutorial")
    fmt.println("  version               - Show version")
    fmt.println("  help                  - This help")
}

// Docs command (stub)
do_docs :: proc() {
    print_colored(GREEN, "Documentation: Visit https://hackeros/docs or read local docs in %s/docs", HACKER_SCRIPT_HOME)
}

// Tutorial command (stub)
do_tutorial :: proc() {
    print_colored(GREEN, "Tutorial: Hello World in HackerScript")
    fmt.println(`func main() [ log"Hello, Hacker!" ]`)
}

// Version command
do_version :: proc() {
    content, ok := read_file(VERSION_FILE)
    if ok {
        print_colored(GREEN, "Version: %s", strings.trim_space(content))
    } else {
        print_colored(RED, "Version file not found")
    }
}

// Main CLI parser
main :: proc() {
    if len(os.args) < 2 {
        do_help()
        return
    }

    cmd := os.args[1]
    args := os.args[2:]

    switch cmd {
    case "build":
        do_build(args)
    case "install":
        if len(args) < 1 {
            print_colored(RED, "Usage: install <lib>")
            return
        }
        do_install(args[0])
    case "remove":
        if len(args) < 1 {
            print_colored(RED, "Usage: remove <lib>")
            return
        }
        do_remove(args[0])
    case "update":
        do_update()
    case "docs":
        do_docs()
    case "tutorial":
        do_tutorial()
    case "version":
        do_version()
    case "help":
        do_help()
    case:
        print_colored(RED, "Unknown command: %s", cmd)
        do_help()
    }
}

// Compile with: odin build virus.odin -out:virus -o:speed -no-bounds-check -build-mode:executable -extra-linker-flags:"-static"
