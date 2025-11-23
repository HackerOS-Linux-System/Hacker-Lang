const std = @import("std");
const utils = @import("utils.zig");

pub const Plugin = struct {
    path: []const u8,
    is_super: bool,
};

pub const ParseResult = struct {
    deps: std.StringHashMap(void),
    libs: std.StringHashMap(void),
    vars_dict: std.StringHashMap([]const u8),
    local_vars: std.StringHashMap([]const u8),
    cmds: std.ArrayList([]const u8),
    includes: std.ArrayList([]const u8),
    binaries: std.ArrayList([]const u8),
    plugins: std.ArrayList(Plugin),
    functions: std.StringHashMap(std.ArrayList([]const u8)),
    errors: std.ArrayList([]const u8),
    config_data: std.StringHashMap([]const u8),
};

pub fn parse_hacker_file(allocator: std.mem.Allocator, file_path: []const u8, verbose: bool) !ParseResult {
    var deps = std.StringHashMap(void).init(allocator);
    var libs = std.StringHashMap(void).init(allocator);
    var vars_dict = std.StringHashMap([]const u8).init(allocator);
    var local_vars = std.StringHashMap([]const u8).init(allocator);
    var cmds = std.ArrayList([]const u8).init(allocator);
    var includes = std.ArrayList([]const u8).init(allocator);
    var binaries = std.ArrayList([]const u8).init(allocator);
    var plugins = std.ArrayList(Plugin).init(allocator);
    var functions = std.StringHashMap(std.ArrayList([]const u8)).init(allocator);
    var errors = std.ArrayList([]const u8).init(allocator);
    var config_data = std.StringHashMap([]const u8).init(allocator);
    var in_config = false;
    var in_comment = false;
    var in_function: ?[]const u8 = null;
    var line_num: u32 = 0;
    const home = std.posix.getenv("HOME") orelse "";
    const hacker_dir = try std.fs.path.join(allocator, &.{ home, utils.HACKER_DIR_SUFFIX });
    defer allocator.free(hacker_dir);
    const console = std.io.getStdOut().writer();
    const file = std.fs.cwd().openFile(file_path, .{}) catch |err| {
        if (err == error.FileNotFound) {
            if (verbose) try console.print("File {s} not found\n", .{file_path});
            try errors.append(try std.fmt.allocPrint(allocator, "File {s} not found", .{file_path}));
            return ParseResult{
                .deps = deps,
                .libs = libs,
                .vars_dict = vars_dict,
                .local_vars = local_vars,
                .cmds = cmds,
                .includes = includes,
                .binaries = binaries,
                .plugins = plugins,
                .functions = functions,
                .errors = errors,
                .config_data = config_data,
            };
        }
        return err;
    };
    defer file.close();
    const reader = file.reader();
    var line_buf: [4096]u8 = undefined;
    while (reader.readUntilDelimiterOrEof(&line_buf, '\n') catch null) |line_slice| {
        line_num += 1;
        const line_trimmed = std.mem.trim(u8, line_slice, " \t\r\n");
        if (line_trimmed.len == 0) continue;
        var line = try allocator.dupe(u8, line_trimmed);
        defer allocator.free(line);
        if (std.mem.eql(u8, line, "!!")) {
            in_comment = !in_comment;
            continue;
        }
        if (in_comment) continue;
        const is_super = std.mem.startsWith(u8, line, "^");
        if (is_super) {
            const new_line = std.mem.trim(u8, line[1..], " \t");
            allocator.free(line);
            line = try allocator.dupe(u8, new_line);
        }
        if (std.mem.eql(u8, line, "[")) {
            if (in_config) {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Nested config section", .{line_num}));
            }
            if (in_function != null) {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Config in function", .{line_num}));
            }
            in_config = true;
            continue;
        } else if (std.mem.eql(u8, line, "]")) {
            if (!in_config) {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Closing ] without [", .{line_num}));
            }
            in_config = false;
            continue;
        }
        if (in_config) {
            if (std.mem.indexOfScalar(u8, line, '=')) |eq_pos| {
                const key = std.mem.trim(u8, line[0..eq_pos], " \t");
                const value = std.mem.trim(u8, line[eq_pos + 1 ..], " \t");
                try config_data.put(try allocator.dupe(u8, key), try allocator.dupe(u8, value));
            }
            continue;
        }
        if (std.mem.eql(u8, line, ":")) {
            if (in_function != null) {
                in_function = null;
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Ending function without start", .{line_num}));
            }
            continue;
        } else if (std.mem.startsWith(u8, line, ":")) {
            const func_name = std.mem.trim(u8, line[1..], " \t");
            if (func_name.len == 0) {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Empty function name", .{line_num}));
                continue;
            }
            if (in_function != null) {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Nested function", .{line_num}));
            }
            const func_name_dupe = try allocator.dupe(u8, func_name);
            try functions.put(func_name_dupe, std.ArrayList([]const u8).init(allocator));
            in_function = func_name_dupe;
            continue;
        } else if (std.mem.startsWith(u8, line, ".")) {
            const func_name = std.mem.trim(u8, line[1..], " \t");
            if (func_name.len == 0) {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Empty function call", .{line_num}));
                continue;
            }
            if (functions.get(func_name)) |func_cmds| {
                var target = if (in_function) |f| functions.getPtr(f).? else &cmds;
                for (func_cmds.items) |c| {
                    try target.append(try allocator.dupe(u8, c));
                }
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Unknown function {s}", .{line_num, func_name}));
            }
            continue;
        }
        if (in_function != null) {
            if (!std.mem.startsWith(u8, line, ">") and !std.mem.startsWith(u8, line, "=") and !std.mem.startsWith(u8, line, "?") and !std.mem.startsWith(u8, line, "&") and !std.mem.startsWith(u8, line, "!") and !std.mem.startsWith(u8, line, "@") and !std.mem.startsWith(u8, line, "$") and !std.mem.startsWith(u8, line, "\\")) {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Invalid in function", .{line_num}));
                continue;
            }
        }
        if (std.mem.startsWith(u8, line, "//")) {
            if (in_function != null) {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Deps not allowed in function", .{line_num}));
                continue;
            }
            const dep = std.mem.trim(u8, line[2..], " \t");
            if (dep.len > 0) {
                _ = try deps.put(try allocator.dupe(u8, dep), {});
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Empty system dependency", .{line_num}));
            }
        } else if (std.mem.startsWith(u8, line, "#")) {
            if (in_function != null) {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Libs not allowed in function", .{line_num}));
                continue;
            }
            const lib = std.mem.trim(u8, line[1..], " \t");
            if (lib.len > 0) {
                const lib_dir = try std.fs.path.join(allocator, &.{ hacker_dir, "libs", lib });
                defer allocator.free(lib_dir);
                const lib_hacker_path = try std.fs.path.join(allocator, &.{ lib_dir, "main.hacker" });
                defer allocator.free(lib_hacker_path);
                const lib_bin_path = try std.fs.path.join(allocator, &.{ hacker_dir, "libs", lib });
                defer allocator.free(lib_bin_path);
                if (std.fs.cwd().access(lib_hacker_path, .{})) |_| {
                    try includes.append(try allocator.dupe(u8, lib));
                    var sub = try parse_hacker_file(allocator, lib_hacker_path, verbose);
                    try utils.mergeHashMaps(void, &deps, sub.deps, allocator);
                    try utils.mergeHashMaps(void, &libs, sub.libs, allocator);
                    try utils.mergeStringHashMaps(&vars_dict, sub.vars_dict, allocator);
                    try utils.mergeStringHashMaps(&local_vars, sub.local_vars, allocator);
                    try cmds.appendSlice(sub.cmds.items);
                    try includes.appendSlice(sub.includes.items);
                    try binaries.appendSlice(sub.binaries.items);
                    try plugins.appendSlice(sub.plugins.items);
                    try utils.mergeFunctionMaps(&functions, sub.functions, allocator);
                    for (sub.errors.items) |sub_err| {
                        try errors.append(try std.fmt.allocPrint(allocator, "In {s}: {s}", .{ lib, sub_err }));
                    }
                    utils.deinitParseResult(&sub, allocator);
                } else |_| {}
                if (std.posix.access(lib_bin_path, std.posix.X_OK)) |_| {
                    try binaries.append(try allocator.dupe(u8, lib_bin_path));
                } else |_| {
                    _ = try libs.put(try allocator.dupe(u8, lib), {});
                }
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Empty library/include", .{line_num}));
            }
        } else if (std.mem.startsWith(u8, line, ">")) {
            const cmd_part = std.mem.trim(u8, if (std.mem.indexOfScalar(u8, line[1..], '!')) |pos| line[1 .. 1 + pos] else line[1..], " \t");
            var cmd = try allocator.dupe(u8, cmd_part);
            if (is_super) {
                const sudo_cmd = try std.fmt.allocPrint(allocator, "sudo {s}", .{cmd});
                allocator.free(cmd);
                cmd = sudo_cmd;
            }
            if (cmd.len > 0) {
                var target = if (in_function) |f| functions.getPtr(f).? else &cmds;
                try target.append(cmd);
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Empty command", .{line_num}));
            }
        } else if (std.mem.startsWith(u8, line, "@")) {
            if (std.mem.indexOfScalar(u8, line[1..], '=')) |eq_pos| {
                const var_name = std.mem.trim(u8, line[1 .. 1 + eq_pos], " \t");
                const value = std.mem.trim(u8, line[1 + eq_pos + 1 ..], " \t");
                if (var_name.len > 0 and value.len > 0) {
                    try vars_dict.put(try allocator.dupe(u8, var_name), try allocator.dupe(u8, value));
                } else {
                    try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Invalid variable", .{line_num}));
                }
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Invalid @ syntax", .{line_num}));
            }
        } else if (std.mem.startsWith(u8, line, "$")) {
            if (std.mem.indexOfScalar(u8, line[1..], '=')) |eq_pos| {
                const var_name = std.mem.trim(u8, line[1 .. 1 + eq_pos], " \t");
                const value = std.mem.trim(u8, line[1 + eq_pos + 1 ..], " \t");
                if (var_name.len > 0 and value.len > 0) {
                    try local_vars.put(try allocator.dupe(u8, var_name), try allocator.dupe(u8, value));
                } else {
                    try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Invalid local variable", .{line_num}));
                }
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Invalid $ syntax", .{line_num}));
            }
        } else if (std.mem.startsWith(u8, line, "\\")) {
            const plugin_name = std.mem.trim(u8, line[1..], " \t");
            if (plugin_name.len > 0) {
                const plugin_dir = try std.fs.path.join(allocator, &.{ hacker_dir, "plugins", plugin_name });
                if (std.posix.access(plugin_dir, std.posix.X_OK)) |_| {
                    try plugins.append(Plugin{ .path = plugin_dir, .is_super = is_super });
                } else |_| {
                    allocator.free(plugin_dir);
                    try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Plugin {s} not found or not executable", .{line_num, plugin_name}));
                }
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Empty plugin name", .{line_num}));
            }
        } else if (std.mem.startsWith(u8, line, "=")) {
            if (std.mem.indexOfScalar(u8, line[1..], '>')) |gt_pos| {
                const num_str = std.mem.trim(u8, line[1 .. 1 + gt_pos], " \t");
                const cmd_part_str = if (std.mem.indexOfScalar(u8, line[1 + gt_pos + 1 ..], '!')) |pos| line[1 + gt_pos + 1 .. 1 + gt_pos + 1 + pos] else line[1 + gt_pos + 1 ..];
                const cmd_part = std.mem.trim(u8, cmd_part_str, " \t");
                const num = std.fmt.parseInt(i32, num_str, 10) catch {
                    try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Invalid loop count", .{line_num}));
                    continue;
                };
                if (num < 0) {
                    try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Negative loop count", .{line_num}));
                    continue;
                }
                var target = if (in_function) |f| functions.getPtr(f).? else &cmds;
                if (cmd_part.len > 0) {
                    var i: i32 = 0;
                    while (i < num) : (i += 1) {
                        var cmd = try allocator.dupe(u8, cmd_part);
                        if (is_super) {
                            const sudo_cmd = try std.fmt.allocPrint(allocator, "sudo {s}", .{cmd});
                            allocator.free(cmd);
                            cmd = sudo_cmd;
                        }
                        try target.append(cmd);
                    }
                } else {
                    try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Empty loop command", .{line_num}));
                }
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Invalid loop syntax", .{line_num}));
            }
        } else if (std.mem.startsWith(u8, line, "?")) {
            if (std.mem.indexOfScalar(u8, line[1..], '>')) |gt_pos| {
                const condition = std.mem.trim(u8, line[1 .. 1 + gt_pos], " \t");
                const cmd_part_str = if (std.mem.indexOfScalar(u8, line[1 + gt_pos + 1 ..], '!')) |pos| line[1 + gt_pos + 1 .. 1 + gt_pos + 1 + pos] else line[1 + gt_pos + 1 ..];
                const cmd_part = std.mem.trim(u8, cmd_part_str, " \t");
                var cmd = cmd_part;
                if (is_super) {
                    cmd = try std.fmt.allocPrint(allocator, "sudo {s}", .{cmd_part});
                    defer allocator.free(cmd);
                }
                if (condition.len > 0 and cmd_part.len > 0) {
                    const if_cmd = try std.fmt.allocPrint(allocator, "if {s}; then {s}; fi", .{ condition, cmd });
                    var target = if (in_function) |f| functions.getPtr(f).? else &cmds;
                    try target.append(if_cmd);
                } else {
                    try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Invalid conditional", .{line_num}));
                }
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Invalid conditional syntax", .{line_num}));
            }
        } else if (std.mem.startsWith(u8, line, "&")) {
            const cmd_part_str = if (std.mem.indexOfScalar(u8, line[1..], '!')) |pos| line[1 .. 1 + pos] else line[1..];
            const cmd_part = std.mem.trim(u8, cmd_part_str, " \t");
            var cmd = try std.fmt.allocPrint(allocator, "{s} &", .{ cmd_part });
            if (is_super) {
                const sudo_cmd = try std.fmt.allocPrint(allocator, "sudo {s}", .{cmd});
                allocator.free(cmd);
                cmd = sudo_cmd;
            }
            if (cmd_part.len > 0) {
                var target = if (in_function) |f| functions.getPtr(f).? else &cmds;
                try target.append(cmd);
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Empty background command", .{line_num}));
            }
        } else if (std.mem.startsWith(u8, line, "!")) {
            // pass
        } else {
            try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Invalid syntax", .{line_num}));
        }
    }
    if (in_config) {
        try errors.append(try allocator.dupe(u8, "Unclosed config section"));
    }
    if (in_comment) {
        try errors.append(try allocator.dupe(u8, "Unclosed comment block"));
    }
    if (in_function != null) {
        try errors.append(try allocator.dupe(u8, "Unclosed function block"));
    }
    if (verbose) {
        var dep_keys = try allocator.alloc([]const u8, deps.count());
        defer allocator.free(dep_keys);
        var i: usize = 0;
        var dep_it = deps.keyIterator();
        while (dep_it.next()) |key| {
            dep_keys[i] = key.*;
            i += 1;
        }
        try console.print("System Deps: {any}\n", .{dep_keys});
        var lib_keys = try allocator.alloc([]const u8, libs.count());
        defer allocator.free(lib_keys);
        i = 0;
        var lib_it = libs.keyIterator();
        while (lib_it.next()) |key| {
            lib_keys[i] = key.*;
            i += 1;
        }
        try console.print("Custom Libs: {any}\n", .{lib_keys});
        try console.print("Vars: {any}\n", .{vars_dict});
        try console.print("Local Vars: {any}\n", .{local_vars});
        try console.print("Cmds: {any}\n", .{cmds.items});
        try console.print("Includes: {any}\n", .{includes.items});
        try console.print("Binaries: {any}\n", .{binaries.items});
        try console.print("Plugins: {any}\n", .{plugins.items});
        try console.print("Functions: {any}\n", .{functions});
        try console.print("Config: {any}\n", .{config_data});
        if (errors.items.len > 0) {
            try console.print("Errors: {any}\n", .{errors.items});
        }
    }
    return ParseResult{
        .deps = deps,
        .libs = libs,
        .vars_dict = vars_dict,
        .local_vars = local_vars,
        .cmds = cmds,
        .includes = includes,
        .binaries = binaries,
        .plugins = plugins,
        .functions = functions,
        .errors = errors,
        .config_data = config_data,
    };
}
