const std = @import("std");
const parse = @import("parse.zig");
pub const HACKER_DIR_SUFFIX = "/.hackeros/hacker-lang";
pub fn deinitParseResult(res: *parse.ParseResult, allocator: std.mem.Allocator) void {
    {
        var it = res.deps.keyIterator();
        while (it.next()) |key| {
            allocator.free(key.*);
        }
        res.deps.deinit();
    }
    {
        var it = res.libs.keyIterator();
        while (it.next()) |key| {
            allocator.free(key.*);
        }
        res.libs.deinit();
    }
    {
        var it = res.vars_dict.iterator();
        while (it.next()) |entry| {
            allocator.free(entry.key_ptr.*);
            allocator.free(entry.value_ptr.*);
        }
        res.vars_dict.deinit();
    }
    {
        for (res.cmds.items) |item| {
            allocator.free(item);
        }
        res.cmds.deinit();
    }
    {
        for (res.includes.items) |item| {
            allocator.free(item);
        }
        res.includes.deinit();
    }
    {
        for (res.binaries.items) |item| {
            allocator.free(item);
        }
        res.binaries.deinit();
    }
    {
        for (res.plugins.items) |item| {
            allocator.free(item);
        }
        res.plugins.deinit();
    }
    {
        for (res.errors.items) |item| {
            allocator.free(item);
        }
        res.errors.deinit();
    }
    {
        var it = res.config_data.iterator();
        while (it.next()) |entry| {
            allocator.free(entry.key_ptr.*);
            allocator.free(entry.value_ptr.*);
        }
        res.config_data.deinit();
    }
}
pub fn mergeHashMaps(comptime V: type, dest: *std.StringHashMap(V), src: std.StringHashMap(V), allocator: std.mem.Allocator) !void {
    var it = src.iterator();
    while (it.next()) |entry| {
        try dest.put(try allocator.dupe(u8, entry.key_ptr.*), entry.value_ptr.*);
    }
}
pub fn mergeStringHashMaps(dest: *std.StringHashMap([]const u8), src: std.StringHashMap([]const u8), allocator: std.mem.Allocator) !void {
    var it = src.iterator();
    while (it.next()) |entry| {
        try dest.put(try allocator.dupe(u8, entry.key_ptr.*), try allocator.dupe(u8, entry.value_ptr.*));
    }
}
