//! BibTeX/BibLaTeX parser.
//!
//! Parses .bib files into normalized Entry structures.

const std = @import("std");
const entry_mod = @import("entry.zig");
const Entry = entry_mod.Entry;

pub const ParseError = error{
    InvalidSyntax,
    UnexpectedEof,
    OutOfMemory,
    InvalidCharacter,
};

/// Parse a BibTeX file and return normalized entries.
pub fn parseFile(allocator: std.mem.Allocator, path: []const u8) ![]Entry {
    const file = try std.fs.cwd().openFile(path, .{});
    defer file.close();

    const content = try file.readToEndAlloc(allocator, 10 * 1024 * 1024);
    defer allocator.free(content);

    return try parseString(allocator, content);
}

/// Parse a BibTeX string and return normalized entries.
pub fn parseString(allocator: std.mem.Allocator, content: []const u8) ![]Entry {
    var entries: std.ArrayList(Entry) = .empty;
    errdefer {
        for (entries.items) |*e| e.deinit();
        entries.deinit(allocator);
    }

    var pos: usize = 0;
    while (pos < content.len) {
        while (pos < content.len) {
            if (std.ascii.isWhitespace(content[pos])) {
                pos += 1;
            } else if (content[pos] == '%') {
                while (pos < content.len and content[pos] != '\n') {
                    pos += 1;
                }
            } else {
                break;
            }
        }

        if (pos >= content.len) break;

        if (content[pos] == '@') {
            if (try parseEntry(allocator, content, &pos)) |parsed_entry| {
                try entries.append(allocator, parsed_entry);
            }
        } else {
            pos += 1;
        }
    }

    return entries.toOwnedSlice(allocator);
}

fn parseEntry(allocator: std.mem.Allocator, content: []const u8, pos: *usize) !?Entry {
    pos.* += 1;

    const type_start = pos.*;
    while (pos.* < content.len and (std.ascii.isAlphanumeric(content[pos.*]) or content[pos.*] == '_')) {
        pos.* += 1;
    }
    const entry_type_raw = content[type_start..pos.*];

    const lower_type = blk: {
        var buf: [32]u8 = undefined;
        const len = @min(entry_type_raw.len, 32);
        for (0..len) |i| {
            buf[i] = std.ascii.toLower(entry_type_raw[i]);
        }
        break :blk buf[0..len];
    };

    if (std.mem.eql(u8, lower_type, "string") or
        std.mem.eql(u8, lower_type, "preamble") or
        std.mem.eql(u8, lower_type, "comment"))
    {
        var depth: i32 = 0;
        while (pos.* < content.len) {
            if (content[pos.*] == '{') depth += 1;
            if (content[pos.*] == '}') {
                depth -= 1;
                if (depth == 0) {
                    pos.* += 1;
                    break;
                }
            }
            pos.* += 1;
        }
        return null;
    }

    while (pos.* < content.len and std.ascii.isWhitespace(content[pos.*])) {
        pos.* += 1;
    }

    if (pos.* >= content.len) return null;
    const open_char = content[pos.*];
    const close_char: u8 = if (open_char == '{') '}' else if (open_char == '(') ')' else return null;
    pos.* += 1;

    while (pos.* < content.len and std.ascii.isWhitespace(content[pos.*])) {
        pos.* += 1;
    }

    const key_start = pos.*;
    while (pos.* < content.len and content[pos.*] != ',' and content[pos.*] != close_char and !std.ascii.isWhitespace(content[pos.*])) {
        pos.* += 1;
    }
    const key = try allocator.dupe(u8, content[key_start..pos.*]);

    while (pos.* < content.len and (std.ascii.isWhitespace(content[pos.*]) or content[pos.*] == ',')) {
        pos.* += 1;
    }

    // Allocate entry_type, freeing key on failure
    const entry_type = allocator.dupe(u8, entry_type_raw) catch |err| {
        allocator.free(key);
        return err;
    };

    var result = Entry{
        .key = key,
        .entry_type = entry_type,
        .allocator = allocator,
    };
    errdefer result.deinit();

    var authors_list: std.ArrayList([]const u8) = .empty;
    defer authors_list.deinit(allocator);

    while (pos.* < content.len and content[pos.*] != close_char) {
        while (pos.* < content.len and std.ascii.isWhitespace(content[pos.*])) {
            pos.* += 1;
        }

        if (pos.* >= content.len or content[pos.*] == close_char) break;

        const field_start = pos.*;
        while (pos.* < content.len and (std.ascii.isAlphanumeric(content[pos.*]) or content[pos.*] == '_' or content[pos.*] == '-')) {
            pos.* += 1;
        }
        const field_name = content[field_start..pos.*];

        while (pos.* < content.len and std.ascii.isWhitespace(content[pos.*])) {
            pos.* += 1;
        }

        if (pos.* >= content.len or content[pos.*] != '=') {
            while (pos.* < content.len and content[pos.*] != ',' and content[pos.*] != close_char) {
                pos.* += 1;
            }
            if (pos.* < content.len and content[pos.*] == ',') pos.* += 1;
            continue;
        }
        pos.* += 1;

        while (pos.* < content.len and std.ascii.isWhitespace(content[pos.*])) {
            pos.* += 1;
        }

        const value = try parseFieldValue(allocator, content, pos);
        defer allocator.free(value);

        while (pos.* < content.len and (std.ascii.isWhitespace(content[pos.*]) or content[pos.*] == ',')) {
            pos.* += 1;
        }

        // Use case-insensitive comparison for field names
        if (std.ascii.eqlIgnoreCase(field_name, "title")) {
            const new_title = try allocator.dupe(u8, value);
            if (result.title) |old| allocator.free(old);
            result.title = new_title;
        } else if (std.ascii.eqlIgnoreCase(field_name, "author")) {
            var iter = std.mem.splitSequence(u8, value, " and ");
            while (iter.next()) |author_str| {
                const trimmed = std.mem.trim(u8, author_str, " \t\r\n");
                if (trimmed.len > 0) {
                    try authors_list.append(allocator, try allocator.dupe(u8, trimmed));
                }
            }
        } else if (std.ascii.eqlIgnoreCase(field_name, "year")) {
            result.year = std.fmt.parseInt(i32, value, 10) catch null;
        } else if (std.ascii.eqlIgnoreCase(field_name, "journal") or std.ascii.eqlIgnoreCase(field_name, "booktitle")) {
            if (result.venue == null) {
                result.venue = try allocator.dupe(u8, value);
            }
        } else if (std.ascii.eqlIgnoreCase(field_name, "doi")) {
            const new_doi = try allocator.dupe(u8, value);
            if (result.doi) |old| allocator.free(old);
            result.doi = new_doi;
        } else if (std.ascii.eqlIgnoreCase(field_name, "eprint")) {
            if (isArxivId(value)) {
                const new_arxiv = try allocator.dupe(u8, value);
                if (result.arxiv_id) |old| allocator.free(old);
                result.arxiv_id = new_arxiv;
            }
        } else if (std.ascii.eqlIgnoreCase(field_name, "url")) {
            const new_url = try allocator.dupe(u8, value);
            if (result.url) |old| allocator.free(old);
            result.url = new_url;

            if (result.arxiv_id == null) {
                if (extractArxivFromUrl(value)) |arxiv| {
                    result.arxiv_id = try allocator.dupe(u8, arxiv);
                }
            }

            if (result.doi == null) {
                if (extractDoiFromUrl(value)) |doi| {
                    result.doi = try allocator.dupe(u8, doi);
                }
            }
        }
    }

    if (pos.* < content.len and content[pos.*] == close_char) {
        pos.* += 1;
    }

    if (authors_list.items.len > 0) {
        result.authors = try authors_list.toOwnedSlice(allocator);
    }

    return result;
}

fn parseFieldValue(allocator: std.mem.Allocator, content: []const u8, pos: *usize) ![]u8 {
    var result: std.ArrayList(u8) = .empty;
    errdefer result.deinit(allocator);

    while (pos.* < content.len) {
        const c = content[pos.*];

        if (c == '"') {
            pos.* += 1;
            while (pos.* < content.len and content[pos.*] != '"') {
                try result.append(allocator, content[pos.*]);
                pos.* += 1;
            }
            if (pos.* < content.len) pos.* += 1;
        } else if (c == '{') {
            pos.* += 1;
            var depth: i32 = 1;
            while (pos.* < content.len and depth > 0) {
                if (content[pos.*] == '{') {
                    depth += 1;
                } else if (content[pos.*] == '}') {
                    depth -= 1;
                    if (depth == 0) break;
                }
                if (depth > 0) {
                    try result.append(allocator, content[pos.*]);
                }
                pos.* += 1;
            }
            if (pos.* < content.len) pos.* += 1;
        } else if (std.ascii.isAlphanumeric(c)) {
            const start = pos.*;
            while (pos.* < content.len and (std.ascii.isAlphanumeric(content[pos.*]) or content[pos.*] == '_')) {
                pos.* += 1;
            }
            try result.appendSlice(allocator, content[start..pos.*]);
        } else if (c == '#') {
            pos.* += 1;
            while (pos.* < content.len and std.ascii.isWhitespace(content[pos.*])) {
                pos.* += 1;
            }
        } else if (c == ',' or c == '}' or c == ')') {
            break;
        } else if (std.ascii.isWhitespace(c)) {
            pos.* += 1;
        } else {
            pos.* += 1;
        }
    }

    return result.toOwnedSlice(allocator);
}

fn isArxivId(s: []const u8) bool {
    const trimmed = std.mem.trim(u8, s, " \t\r\n");

    if (std.mem.indexOf(u8, trimmed, "/")) |slash_pos| {
        const after_slash = trimmed[slash_pos + 1 ..];
        for (after_slash) |c| {
            if (!std.ascii.isDigit(c)) return false;
        }
        return after_slash.len > 0;
    }

    if (std.mem.indexOf(u8, trimmed, ".")) |dot_pos| {
        if (dot_pos != 4) return false;
        for (trimmed[0..4]) |c| {
            if (!std.ascii.isDigit(c)) return false;
        }
        var i: usize = dot_pos + 1;
        while (i < trimmed.len and std.ascii.isDigit(trimmed[i])) {
            i += 1;
        }
        if (i == dot_pos + 1) return false;
        if (i < trimmed.len and trimmed[i] == 'v') {
            i += 1;
            while (i < trimmed.len and std.ascii.isDigit(trimmed[i])) {
                i += 1;
            }
        }
        return i == trimmed.len;
    }

    return false;
}

fn extractArxivFromUrl(url: []const u8) ?[]const u8 {
    if (std.mem.indexOf(u8, url, "arxiv.org")) |_| {
        const patterns = [_][]const u8{ "/abs/", "/pdf/" };
        for (patterns) |pattern| {
            if (std.mem.indexOf(u8, url, pattern)) |idx| {
                const start = idx + pattern.len;
                var end = start;
                while (end < url.len and (std.ascii.isAlphanumeric(url[end]) or url[end] == '.' or url[end] == '/' or url[end] == 'v')) {
                    end += 1;
                }
                var id = url[start..end];
                if (std.mem.endsWith(u8, id, ".pdf")) {
                    id = id[0 .. id.len - 4];
                }
                if (isArxivId(id)) {
                    return id;
                }
            }
        }
    }
    return null;
}

fn extractDoiFromUrl(url: []const u8) ?[]const u8 {
    if (std.mem.indexOf(u8, url, "doi.org/")) |idx| {
        const doi = url[idx + 8 ..];
        if (std.mem.startsWith(u8, doi, "10.")) {
            return doi;
        }
    }
    return null;
}

test "isArxivId" {
    try std.testing.expect(isArxivId("2301.12345"));
    try std.testing.expect(isArxivId("2301.12345v1"));
    try std.testing.expect(isArxivId("hep-th/9901001"));
    try std.testing.expect(!isArxivId("not-an-arxiv-id"));
    try std.testing.expect(!isArxivId("10.1234/example"));
}

test "parseString simple" {
    const allocator = std.testing.allocator;
    const bib =
        \\@article{smith2021,
        \\    author = {John Smith and Jane Doe},
        \\    title = {A Great Paper},
        \\    journal = {Nature},
        \\    year = {2021},
        \\    doi = {10.1234/example}
        \\}
    ;

    const entries = try parseString(allocator, bib);
    defer {
        for (entries) |*e| {
            var bib_entry = @constCast(e);
            bib_entry.deinit();
        }
        allocator.free(entries);
    }

    try std.testing.expectEqual(@as(usize, 1), entries.len);
    try std.testing.expectEqualStrings("smith2021", entries[0].key);
    try std.testing.expectEqualStrings("A Great Paper", entries[0].title.?);
    try std.testing.expectEqual(@as(i32, 2021), entries[0].year.?);
    try std.testing.expectEqual(@as(usize, 2), entries[0].authors.len);
}
