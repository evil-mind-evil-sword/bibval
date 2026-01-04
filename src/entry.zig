//! Entry data structures for bibliographic validation.
//!
//! This module defines the normalized entry format used for comparison
//! across different academic databases.

const std = @import("std");

/// Normalized bibliography entry for comparison across different sources.
pub const Entry = struct {
    /// Citation key from the bib file
    key: []const u8,
    /// Entry type (article, inproceedings, book, etc.)
    entry_type: []const u8,
    /// Paper title
    title: ?[]const u8 = null,
    /// List of authors
    authors: []const []const u8 = &.{},
    /// Publication year
    year: ?i32 = null,
    /// Journal or conference venue
    venue: ?[]const u8 = null,
    /// DOI identifier
    doi: ?[]const u8 = null,
    /// ArXiv identifier (e.g., "2301.12345")
    arxiv_id: ?[]const u8 = null,
    /// URL
    url: ?[]const u8 = null,

    allocator: ?std.mem.Allocator = null,

    pub fn deinit(self: *Entry) void {
        if (self.allocator) |alloc| {
            if (self.key.len > 0) alloc.free(self.key);
            if (self.entry_type.len > 0) alloc.free(self.entry_type);
            if (self.title) |t| alloc.free(t);
            for (self.authors) |a| alloc.free(a);
            if (self.authors.len > 0) alloc.free(self.authors);
            if (self.venue) |v| alloc.free(v);
            if (self.doi) |d| alloc.free(d);
            if (self.arxiv_id) |a| alloc.free(a);
            if (self.url) |u| alloc.free(u);
        }
    }

    /// Normalize title for comparison (lowercase, remove extra whitespace)
    pub fn normalizedTitle(self: *const Entry, allocator: std.mem.Allocator) !?[]u8 {
        if (self.title) |t| {
            return try normalizeString(allocator, t);
        }
        return null;
    }
};

/// Result from an external API validation.
pub const ValidationResult = struct {
    /// Which API this result came from
    source: ApiSource,
    /// The matched entry from the API
    matched_entry: ?Entry = null,
    /// Confidence score (0.0 to 1.0)
    confidence: f64,
    /// List of discrepancies found
    discrepancies: []const Discrepancy = &.{},

    allocator: ?std.mem.Allocator = null,

    pub fn deinit(self: *ValidationResult) void {
        if (self.allocator) |alloc| {
            if (self.matched_entry) |*e| {
                var entry_copy = e.*;
                entry_copy.deinit();
            }
            for (self.discrepancies) |*d| {
                var disc = @constCast(d);
                disc.deinit();
            }
            if (self.discrepancies.len > 0) {
                alloc.free(self.discrepancies);
            }
        }
    }
};

/// API source identifier.
pub const ApiSource = enum {
    crossref,
    dblp,
    semantic_scholar,
    openalex,

    pub fn name(self: ApiSource) []const u8 {
        return switch (self) {
            .crossref => "CrossRef",
            .dblp => "DBLP",
            .semantic_scholar => "Semantic Scholar",
            .openalex => "OpenAlex",
        };
    }
};

/// Discrepancy between local and remote entries.
pub const Discrepancy = struct {
    field: DiscrepancyField,
    severity: Severity,
    local_value: []const u8,
    remote_value: []const u8,
    message: []const u8,

    allocator: ?std.mem.Allocator = null,

    pub fn deinit(self: *Discrepancy) void {
        if (self.allocator) |alloc| {
            if (self.local_value.len > 0) alloc.free(self.local_value);
            if (self.remote_value.len > 0) alloc.free(self.remote_value);
            if (self.message.len > 0) alloc.free(self.message);
        }
    }
};

/// Fields that can have discrepancies.
pub const DiscrepancyField = enum {
    title,
    authors,
    year,
    venue,
    doi,

    pub fn name(self: DiscrepancyField) []const u8 {
        return switch (self) {
            .title => "Title",
            .authors => "Authors",
            .year => "Year",
            .venue => "Venue",
            .doi => "DOI",
        };
    }
};

/// Severity levels for discrepancies.
pub const Severity = enum {
    info,
    warning,
    @"error",

    pub fn name(self: Severity) []const u8 {
        return switch (self) {
            .info => "INFO",
            .warning => "WARN",
            .@"error" => "ERROR",
        };
    }

    pub fn order(self: Severity) u8 {
        return switch (self) {
            .info => 0,
            .warning => 1,
            .@"error" => 2,
        };
    }
};

/// Normalize a string for comparison: lowercase, collapse whitespace, remove punctuation.
pub fn normalizeString(allocator: std.mem.Allocator, s: []const u8) ![]u8 {
    var result: std.ArrayList(u8) = .empty;
    errdefer result.deinit(allocator);

    var in_whitespace = false;
    for (s) |c| {
        const lower = std.ascii.toLower(c);
        if (std.ascii.isAlphanumeric(lower)) {
            if (in_whitespace and result.items.len > 0) {
                try result.append(allocator, ' ');
            }
            try result.append(allocator, lower);
            in_whitespace = false;
        } else if (std.ascii.isWhitespace(c)) {
            in_whitespace = true;
        }
    }

    return result.toOwnedSlice(allocator);
}

test "normalizeString" {
    const allocator = std.testing.allocator;

    const result = try normalizeString(allocator, "  Hello,   WORLD!  ");
    defer allocator.free(result);
    try std.testing.expectEqualStrings("hello world", result);
}
