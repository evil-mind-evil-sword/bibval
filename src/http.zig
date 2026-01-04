//! HTTP client wrapper for API requests.

const std = @import("std");

pub const HttpError = error{
    RequestFailed,
    RateLimited,
    NotFound,
    ParseError,
    OutOfMemory,
    ConnectionRefused,
    Timeout,
    InvalidUrl,
};

/// HTTP client for making API requests.
pub const Client = struct {
    allocator: std.mem.Allocator,
    user_agent: []const u8,

    pub fn init(allocator: std.mem.Allocator, user_agent: []const u8) Client {
        return .{
            .allocator = allocator,
            .user_agent = user_agent,
        };
    }

    /// Make a GET request and return the response body.
    pub fn get(self: *Client, url: []const u8) ![]u8 {
        const uri = std.Uri.parse(url) catch return HttpError.InvalidUrl;

        var client: std.http.Client = .{ .allocator = self.allocator };
        defer client.deinit();

        // Create request
        var req = client.request(.GET, uri, .{
            .headers = .{
                .user_agent = .{ .override = self.user_agent },
            },
        }) catch |err| {
            return switch (err) {
                error.ConnectionRefused => HttpError.ConnectionRefused,
                error.ConnectionTimedOut => HttpError.Timeout,
                else => HttpError.RequestFailed,
            };
        };
        defer req.deinit();

        // Send request (no body for GET)
        req.sendBodiless() catch return HttpError.RequestFailed;

        // Receive response headers
        var redirect_buf: [8 * 1024]u8 = undefined;
        var response = req.receiveHead(&redirect_buf) catch return HttpError.RequestFailed;

        // Check status code
        if (response.head.status == .not_found) {
            return HttpError.NotFound;
        }
        if (response.head.status == .too_many_requests) {
            return HttpError.RateLimited;
        }
        if (@intFromEnum(response.head.status) >= 400) {
            return HttpError.RequestFailed;
        }

        // Read response body
        var transfer_buf: [4096]u8 = undefined;
        const reader = response.reader(&transfer_buf);

        const body = reader.allocRemaining(self.allocator, .limited(10 * 1024 * 1024)) catch return HttpError.RequestFailed;
        return body;
    }
};

/// URL encode a string.
pub fn urlEncode(allocator: std.mem.Allocator, input: []const u8) ![]u8 {
    var result: std.ArrayList(u8) = .empty;
    errdefer result.deinit(allocator);

    for (input) |c| {
        if (std.ascii.isAlphanumeric(c) or c == '-' or c == '_' or c == '.' or c == '~') {
            try result.append(allocator, c);
        } else if (c == ' ') {
            try result.append(allocator, '+');
        } else {
            try result.appendSlice(allocator, &.{ '%', hexChar(c >> 4), hexChar(c & 0xf) });
        }
    }

    return result.toOwnedSlice(allocator);
}

fn hexChar(n: u8) u8 {
    return if (n < 10) '0' + n else 'A' + n - 10;
}

test "urlEncode" {
    const allocator = std.testing.allocator;

    const result = try urlEncode(allocator, "hello world!");
    defer allocator.free(result);
    try std.testing.expectEqualStrings("hello+world%21", result);
}
