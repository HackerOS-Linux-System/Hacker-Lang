const std = @import("std");
const parse = @import("parse.zig");
const utils = @import("utils.zig");
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
    var res = try parse.parse_hacker_file(allocator, file_path.?, verbose);
    defer utils.deinitParseResult(&res, allocator);
    try outputJson(res);
}
fn outputJson(res: parse.ParseResult) !void {
    const stdout = std.io.getStdOut().writer();
    try stdout.print("{{", .{});
    try stdout.print("\"deps\":[", .{});
    var first = true;
    var dep_it = res.deps.keyIterator();
    while (dep_it.next()) |key| {
        if (!first) try stdout.print(",", .{});
        first = false;
        try std.json.encodeJsonString(key.*, .{}, stdout);
    }
    try stdout.print("],", .{});
    try stdout.print("\"libs\":[", .{});
    first = true;
    var lib_it = res.libs.keyIterator();
    while (lib_it.next()) |key| {
        if (!first) try stdout.print(",", .{});
        first = false;
        try std.json.encodeJsonString(key.*, .{}, stdout);
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
    try stdout.print("\"local_vars\":{{", .{});
    first = true;
    var local_vars_it = res.local_vars.iterator();
    while (local_vars_it.next()) |entry| {
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
    try stdout.print("\"plugins\":[", .{});
    first = true;
    for (res.plugins.items) |p| {
        if (!first) try stdout.print(",", .{});
        first = false;
        try stdout.print("{{", .{});
        try stdout.print("\"path\":", .{});
        try std.json.encodeJsonString(p.path, .{}, stdout);
        try stdout.print(",\"super\":{}", .{p.is_super});
        try stdout.print("}}", .{});
    }
    try stdout.print("],", .{});
    try stdout.print("\"functions\":{{", .{});
    first = true;
    var func_it = res.functions.iterator();
    while (func_it.next()) |entry| {
        if (!first) try stdout.print(",", .{});
        first = false;
        try std.json.encodeJsonString(entry.key_ptr.*, .{}, stdout);
        try stdout.print(":[", .{});
        var first2 = true;
        for (entry.value_ptr.items) |c| {
            if (!first2) try stdout.print(",", .{});
            first2 = false;
            try std.json.encodeJsonString(c, .{}, stdout);
        }
        try stdout.print("]", .{});
    }
    try stdout.print("}},", .{});
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
