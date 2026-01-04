//! Response caching for API calls.

const std = @import("std");

pub const CacheError = error{
    CreateDirFailed,
    IoError,
    OutOfMemory,
};

const CACHE_TTL_SECS: i64 = 86400 * 7; // 7 days

/// Simple file-based cache for API responses.
pub const Cache = struct {
    cache_dir: []const u8,
    enabled: bool,
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator, enabled: bool) !Cache {
        const home = std.process.getEnvVarOwned(allocator, "HOME") catch |err| switch (err) {
            error.EnvironmentVariableNotFound => try allocator.dupe(u8, "/tmp"),
            else => return CacheError.OutOfMemory,
        };
        defer allocator.free(home);

        const cache_dir = try std.fs.path.join(allocator, &.{ home, ".cache", "bibval" });

        if (enabled) {
            std.fs.makeDirAbsolute(cache_dir) catch |err| switch (err) {
                error.PathAlreadyExists => {},
                else => {
                    const parent = std.fs.path.dirname(cache_dir) orelse return CacheError.CreateDirFailed;
                    std.fs.cwd().makePath(parent) catch return CacheError.CreateDirFailed;
                    std.fs.makeDirAbsolute(cache_dir) catch return CacheError.CreateDirFailed;
                },
            };
        }

        return .{
            .cache_dir = cache_dir,
            .enabled = enabled,
            .allocator = allocator,
        };
    }

    pub fn deinit(self: *Cache) void {
        self.allocator.free(self.cache_dir);
    }

    /// Generate cache key from API name and query.
    fn cacheKey(self: *Cache, api: []const u8, query: []const u8) ![]u8 {
        var hasher = std.crypto.hash.sha2.Sha256.init(.{});
        hasher.update(query);
        var hash: [32]u8 = undefined;
        hasher.final(&hash);

        // Convert first 16 bytes to hex (128-bit key, ample for collision resistance)
        var hash_hex: [32]u8 = undefined;
        for (hash[0..16], 0..) |byte, i| {
            hash_hex[i * 2] = "0123456789abcdef"[byte >> 4];
            hash_hex[i * 2 + 1] = "0123456789abcdef"[byte & 0xf];
        }

        const filename = try std.fmt.allocPrint(self.allocator, "{s}_{s}.json", .{ api, hash_hex });
        defer self.allocator.free(filename);

        return std.fs.path.join(self.allocator, &.{ self.cache_dir, filename });
    }

    /// Get a cached response if it exists and is not expired.
    pub fn get(self: *Cache, api: []const u8, query: []const u8) ?[]u8 {
        if (!self.enabled) return null;

        const path = self.cacheKey(api, query) catch return null;
        defer self.allocator.free(path);

        const file = std.fs.openFileAbsolute(path, .{}) catch return null;
        defer file.close();

        const stat = file.stat() catch return null;

        const now = std.time.timestamp();
        const mtime: i64 = @intCast(@divFloor(stat.mtime, std.time.ns_per_s));
        if (now - mtime > CACHE_TTL_SECS) {
            std.fs.deleteFileAbsolute(path) catch {};
            return null;
        }

        return file.readToEndAlloc(self.allocator, 10 * 1024 * 1024) catch null;
    }

    /// Store a response in the cache.
    pub fn set(self: *Cache, api: []const u8, query: []const u8, value: []const u8) !void {
        if (!self.enabled) return;

        const path = try self.cacheKey(api, query);
        defer self.allocator.free(path);

        const file = try std.fs.createFileAbsolute(path, .{});
        defer file.close();

        try file.writeAll(value);
    }
};
