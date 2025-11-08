const std = @import("std");

const HACKER_DIR_SUFFIX = "/.hackeros/hacker-lang";

fn parse_hacker_file(allocator: std.mem.Allocator, file_path: []const u8, verbose: bool) !struct {
    deps: std.StringHashSet,
    libs: std.StringHashSet,
    vars_dict: std.StringHashMap([]const u8),
    cmds: std.ArrayList([]const u8),
    includes: std.ArrayList([]const u8),
    binaries: std.ArrayList([]const u8),
    errors: std.ArrayList([]const u8),
    config_data: std.StringHashMap([]const u8),
} {
    var deps = std.StringHashSet.init(allocator);
    var libs = std.StringHashSet.init(allocator);
    var vars_dict = std.StringHashMap([]const u8).init(allocator);
    var cmds = std.ArrayList([]const u8).init(allocator);
    var includes = std.ArrayList([]const u8).init(allocator);
    var binaries = std.ArrayList([]const u8).init(allocator);
    var errors = std.ArrayList([]const u8).init(allocator);
    var config_data = std.StringHashMap([]const u8).init(allocator);

    var in_config = false;
    var line_num: u32 = 0;

    const home = std.os.getenv("HOME") orelse "";
    const hacker_dir = try std.fs.path.join(allocator, &.{ home, HACKER_DIR_SUFFIX });
    defer allocator.free(hacker_dir);

    const console = std.io.getStdOut().writer();

    const file = std.fs.cwd().openFile(file_path, .{}) catch |err| {
        if (err == error.FileNotFound) {
            if (verbose) try console.print("File {s} not found\n", .{file_path});
            try errors.append(try std.fmt.allocPrint(allocator, "File {s} not found", .{file_path}));
            return .{
                .deps = deps,
                .libs = libs,
                .vars_dict = vars_dict,
                .cmds = cmds,
                .includes = includes,
                .binaries = binaries,
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
        const line_trimmed = std.mem.trim(u8, line_slice orelse break, " \t\r\n");
        if (line_trimmed.len == 0) continue;

        const line = try allocator.dupe(u8, line_trimmed);
        defer allocator.free(line);

        if (std.mem.eql(u8, line, "[")) {
            if (in_config) {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Nested config section", .{line_num}));
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

        if (std.mem.startsWith(u8, line, "//")) {
            const dep = std.mem.trim(u8, line[2..], " \t");
            if (dep.len > 0) {
                _ = try deps.insert(try allocator.dupe(u8, dep));
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Empty system dependency", .{line_num}));
            }
        } else if (std.mem.startsWith(u8, line, "#")) {
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
                    const sub = try parse_hacker_file(allocator, lib_hacker_path, verbose);
                    for (sub.deps.keys()) |sub_dep| {
                        _ = try deps.insert(sub_dep);
                    }
                    for (sub.libs.keys()) |sub_lib| {
                        _ = try libs.insert(sub_lib);
                    }
                    {
                        var sub_it = sub.vars_dict.iterator();
                        while (sub_it.next()) |entry| {
                            try vars_dict.put(entry.key_ptr.*, entry.value_ptr.*);
                        }
                    }
                    try cmds.appendSlice(sub.cmds.items);
                    try includes.appendSlice(sub.includes.items);
                    try binaries.appendSlice(sub.binaries.items);
                    for (sub.errors.items) |sub_err| {
                        try errors.append(try std.fmt.allocPrint(allocator, "In {s}: {s}", .{ lib, sub_err }));
                    }
                    // Deinit sub resources
                    sub.deps.deinit();
                    sub.libs.deinit();
                    sub.vars_dict.deinit();
                    sub.cmds.deinit();
                    sub.includes.deinit();
                    sub.binaries.deinit();
                    sub.errors.deinit();
                    sub.config_data.deinit();
                } else |_| {} // ignore access error for now

                if (std.fs.cwd().access(lib_bin_path, .{ .mode = .execute })) |_| {
                    try binaries.append(try allocator.dupe(u8, lib_bin_path));
                } else |_| {
                    _ = try libs.insert(try allocator.dupe(u8, lib));
                }
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Empty library/include", .{line_num}));
            }
        } else if (std.mem.startsWith(u8, line, ">")) {
            const cmd_part = std.mem.trim(u8, if (std.mem.indexOfScalar(u8, line[1..], '!')) |pos| line[1 .. 1 + pos] else line[1..], " \t");
            if (cmd_part.len > 0) {
                try cmds.append(try allocator.dupe(u8, cmd_part));
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
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Missing = in variable", .{line_num}));
            }
        } else if (std.mem.startsWith(u8, line, "=")) {
            if (std.mem.indexOfScalar(u8, line[1..], '>')) |gt_pos| {
                const num_str = std.mem.trim(u8, line[1 .. 1 + gt_pos], " \t");
                const cmd_part = std.mem.trim(u8, if (std.mem.indexOfScalar(u8, line[1 + gt_pos + 1 ..], '!')) |pos| line[1 + gt_pos + 1 .. 1 + gt_pos + 1 + pos] else line[1 + gt_pos + 1 ..], " \t");
                const num = std.fmt.parseInt(i32, num_str, 10) catch {
                    try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Invalid loop count", .{line_num}));
                    continue;
                };
                if (num < 0) {
                    try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Negative loop count", .{line_num}));
                    continue;
                }
                if (cmd_part.len > 0) {
                    var i: i32 = 0;
                    while (i < num) : (i += 1) {
                        try cmds.append(try allocator.dupe(u8, cmd_part));
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
                const cmd_part = std.mem.trim(u8, if (std.mem.indexOfScalar(u8, line[1 + gt_pos + 1 ..], '!')) |pos| line[1 + gt_pos + 1 .. 1 + gt_pos + 1 + pos] else line[1 + gt_pos + 1 ..], " \t");
                if (condition.len > 0 and cmd_part.len > 0) {
                    const if_cmd = try std.fmt.allocPrint(allocator, "if {s}; then {s}; fi", .{ condition, cmd_part });
                    try cmds.append(if_cmd);
                } else {
                    try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Invalid conditional", .{line_num}));
                }
            } else {
                try errors.append(try std.fmt.allocPrint(allocator, "Line {d}: Invalid conditional syntax", .{line_num}));
            }
        } else if (std.mem.startsWith(u8, line, "&")) {
            const cmd_part = std.mem.trim(u8, if (std.mem.indexOfScalar(u8, line[1..], '!')) |pos| line[1 .. 1 + pos] else line[1..], " \t");
            if (cmd_part.len > 0) {
                const bg_cmd = try std.fmt.allocPrint(allocator, "{s} &", .{ cmd_part });
                try cmds.append(bg_cmd);
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

    if (verbose) {
        try console.print("System Deps: {any}\n", .{deps.keys()});
        try console.print("Custom Libs: {any}\n", .{libs.keys()});
        try console.print("Vars: {any}\n", .{vars_dict.unmanaged});
        try console.print("Cmds: {any}\n", .{cmds.items});
        try console.print("Includes: {any}\n", .{includes.items});
        try console.print("Binaries: {any}\n", .{binaries.items});
        try console.print("Config: {any}\n", .{config_data.unmanaged});
        if (errors.items.len > 0) {
            try console.print("Errors: {any}\n", .{errors.items});
        }
    }

    return .{
        .deps = deps,
        .libs = libs,
        .vars_dict = vars_dict,
        .cmds = cmds,
        .includes = includes,
        .binaries = binaries,
        .errors = errors,
        .config_data = config_data,
    };
}

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    var verbose = false;
    var file_path: ?[]const u8 = null;

    for (std.os.argv[1..]) |arg_ptr| {
        const arg = std.mem.span(arg_ptr);
        if (std.mem.eql(u8, arg, "--verbose")) {
            verbose = true;
        } else if (file_path == null) {
            file_path = arg;
        } else {
            try std.io.getStdErr().writer().print("Usage: hacker-parser [--verbose] <file>\n", .{});
            std.process.exit(1);
        }
    }

    if (file_path == null) {
        try std.io.getStdErr().writer().print("Usage: hacker-parser [--verbose] <file>\n", .{});
        std.process.exit(1);
    }

    const res = try parse_hacker_file(allocator, file_path.?, verbose);

    defer res.deps.deinit();
    defer res.libs.deinit();
    defer res.vars_dict.deinit();
    defer res.cmds.deinit();
    defer res.includes.deinit();
    defer res.binaries.deinit();
    defer res.errors.deinit();
    defer res.config_data.deinit();

    var deps_list = std.ArrayList([]const u8).init(allocator);
    defer deps_list.deinit();
    for (res.deps.keys()) |d| try deps_list.append(d);

    var libs_list = std.ArrayList([]const u8).init(allocator);
    defer libs_list.deinit();
    for (res.libs.keys()) |l| try libs_list.append(l);

    const stdout = std.io.getStdOut().writer();

    try stdout.print("{{", .{});
    try stdout.print("\"deps\":[", .{});
    var first = true;
    for (deps_list.items) |d| {
        if (!first) try stdout.print(",", .{});
        first = false;
        try std.json.encodeJsonString(d, .{}, stdout);
    }
    try stdout.print("],", .{});
    try stdout.print("\"libs\":[", .{});
    first = true;
    for (libs_list.items) |l| {
        if (!first) try stdout.print(",", .{});
        first = false;
        try std.json.encodeJsonString(l, .{}, stdout);
    }
    try stdout.print("],", .{});
    try stdout.print("\"vars\":{{", .{});
    first = true;
    var vars_it = res.vars_dict.iterator();
    while (vars_it.next()) |entry| {
        if (!first) try stdout.print(",", .{});
        first = false;
        try std.json.encodeJsonString(entry.key_ptr.*, .{}, stdout);
        try stdout.print(":", .{});
        try std.json.encodeJsonString(entry.value_ptr.*, .{}, stdout);
    }
    try stdout.print("}},", .{});
    try stdout.print("\"cmds\":[", .{});
    first = true;
    for (res.cmds.items) |c| {
        if (!first) try stdout.print(",", .{});
        first = false;
        try std.json.encodeJsonString(c, .{}, stdout);
    }
    try stdout.print("],", .{});
    try stdout.print("\"includes\":[", .{});
    first = true;
    for (res.includes.items) |i| {
        if (!first) try stdout.print(",", .{});
        first = false;
        try std.json.encodeJsonString(i, .{}, stdout);
    }
    try stdout.print("],", .{});
    try stdout.print("\"binaries\":[", .{});
    first = true;
    for (res.binaries.items) |b| {
        if (!first) try stdout.print(",", .{});
        first = false;
        try std.json.encodeJsonString(b, .{}, stdout);
    }
    try stdout.print("],", .{});
    try stdout.print("\"errors\":[", .{});
    first = true;
    for (res.errors.items) |e| {
        if (!first) try stdout.print(",", .{});
        first = false;
        try std.json.encodeJsonString(e, .{}, stdout);
    }
    try stdout.print("],", .{});
    try stdout.print("\"config\":{{", .{});
    first = true;
    var config_it = res.config_data.iterator();
    while (config_it.next()) |entry| {
        if (!first) try stdout.print(",", .{});
        first = false;
        try std.json.encodeJsonString(entry.key_ptr.*, .{}, stdout);
        try stdout.print(":", .{});
        try std.json.encodeJsonString(entry.value_ptr.*, .{}, stdout);
    }
    try stdout.print("}}", .{});
    try stdout.print("}}\n", .{});
}
