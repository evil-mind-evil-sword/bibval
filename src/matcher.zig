//! String matching and entry comparison utilities.
//!
//! Implements Jaro-Winkler similarity and entry comparison logic.

const std = @import("std");
const entry = @import("entry.zig");
const Entry = entry.Entry;
const Discrepancy = entry.Discrepancy;
const DiscrepancyField = entry.DiscrepancyField;
const Severity = entry.Severity;
const normalizeString = entry.normalizeString;

/// Threshold for title similarity (0.0 to 1.0)
pub const TITLE_MATCH_THRESHOLD: f64 = 0.85;
pub const TITLE_WARNING_THRESHOLD: f64 = 0.90;

/// Threshold for author name similarity
pub const AUTHOR_MATCH_THRESHOLD: f64 = 0.80;

/// Maximum year difference for a valid match
pub const MAX_YEAR_DIFFERENCE: i32 = 2;

/// Minimum author overlap ratio for a valid match
pub const MIN_AUTHOR_OVERLAP: f64 = 0.3;

/// Calculate Jaro similarity between two strings.
/// Uses dynamic allocation to support strings of any length.
pub fn jaroSimilarity(allocator: std.mem.Allocator, s1: []const u8, s2: []const u8) !f64 {
    if (s1.len == 0 and s2.len == 0) return 1.0;
    if (s1.len == 0 or s2.len == 0) return 0.0;

    const match_distance = @max(s1.len, s2.len) / 2;
    if (match_distance == 0) {
        return if (s1.len == 1 and s2.len == 1 and s1[0] == s2[0]) 1.0 else 0.0;
    }

    // Dynamically allocate match tracking arrays to support any string length
    const s1_matches = try allocator.alloc(bool, s1.len);
    defer allocator.free(s1_matches);
    @memset(s1_matches, false);

    const s2_matches = try allocator.alloc(bool, s2.len);
    defer allocator.free(s2_matches);
    @memset(s2_matches, false);

    var matches: usize = 0;
    var transpositions: usize = 0;

    // Find matches
    for (s1, 0..) |c1, i| {
        const start = if (i >= match_distance) i - match_distance else 0;
        const end = @min(i + match_distance + 1, s2.len);

        for (start..end) |j| {
            if (s2_matches[j] or s2[j] != c1) continue;
            s1_matches[i] = true;
            s2_matches[j] = true;
            matches += 1;
            break;
        }
    }

    if (matches == 0) return 0.0;

    // Count transpositions
    var k: usize = 0;
    for (s1, 0..) |_, i| {
        if (!s1_matches[i]) continue;
        while (!s2_matches[k]) k += 1;
        if (s1[i] != s2[k]) transpositions += 1;
        k += 1;
    }

    const m: f64 = @floatFromInt(matches);
    const t: f64 = @floatFromInt(transpositions / 2);
    const len1: f64 = @floatFromInt(s1.len);
    const len2: f64 = @floatFromInt(s2.len);

    return (m / len1 + m / len2 + (m - t) / m) / 3.0;
}

/// Calculate Jaro-Winkler similarity between two strings.
pub fn jaroWinklerSimilarity(allocator: std.mem.Allocator, s1: []const u8, s2: []const u8) !f64 {
    const jaro = try jaroSimilarity(allocator, s1, s2);

    // Find common prefix (up to 4 characters)
    var prefix_len: usize = 0;
    const max_prefix = @min(@min(s1.len, s2.len), 4);
    while (prefix_len < max_prefix and s1[prefix_len] == s2[prefix_len]) {
        prefix_len += 1;
    }

    const p: f64 = 0.1; // Scaling factor
    const l: f64 = @floatFromInt(prefix_len);

    return jaro + l * p * (1.0 - jaro);
}

/// Calculate title similarity between two entries.
pub fn titleSimilarity(allocator: std.mem.Allocator, a: *const Entry, b: *const Entry) !f64 {
    if (a.title == null or b.title == null) return 0.0;

    const norm_a = try normalizeString(allocator, a.title.?);
    defer allocator.free(norm_a);
    const norm_b = try normalizeString(allocator, b.title.?);
    defer allocator.free(norm_b);

    return jaroWinklerSimilarity(allocator, norm_a, norm_b);
}

/// Check if years are within acceptable range.
pub fn yearsCompatible(a: *const Entry, b: *const Entry) bool {
    if (a.year == null or b.year == null) return true;
    const diff = if (a.year.? > b.year.?) a.year.? - b.year.? else b.year.? - a.year.?;
    return diff <= MAX_YEAR_DIFFERENCE;
}

/// Calculate author overlap ratio.
pub fn authorOverlap(allocator: std.mem.Allocator, local: *const Entry, remote: *const Entry) !f64 {
    if (local.authors.len == 0 or remote.authors.len == 0) return 1.0;

    var matches: usize = 0;
    for (local.authors) |local_author| {
        const local_norm = try normalizeString(allocator, local_author);
        defer allocator.free(local_norm);

        for (remote.authors) |remote_author| {
            const remote_norm = try normalizeString(allocator, remote_author);
            defer allocator.free(remote_norm);

            const full_sim = try jaroWinklerSimilarity(allocator, local_norm, remote_norm);

            // Also check last names
            const local_last = lastWord(local_norm);
            const remote_last = lastWord(remote_norm);
            const last_name_sim = try jaroWinklerSimilarity(allocator, local_last, remote_last);

            if (full_sim >= AUTHOR_MATCH_THRESHOLD or last_name_sim >= 0.9) {
                matches += 1;
                break;
            }
        }
    }

    return @as(f64, @floatFromInt(matches)) / @as(f64, @floatFromInt(local.authors.len));
}

fn lastWord(s: []const u8) []const u8 {
    var i = s.len;
    while (i > 0 and s[i - 1] != ' ') {
        i -= 1;
    }
    return s[i..];
}

/// Calculate a combined match score.
pub fn matchScore(allocator: std.mem.Allocator, target: *const Entry, candidate: *const Entry) !f64 {
    const title_sim = try titleSimilarity(allocator, target, candidate);

    if (title_sim < TITLE_MATCH_THRESHOLD) return 0.0;
    if (!yearsCompatible(target, candidate)) return 0.0;

    const author_sim = try authorOverlap(allocator, target, candidate);

    if (target.authors.len > 0 and candidate.authors.len > 0 and author_sim < MIN_AUTHOR_OVERLAP) {
        return 0.0;
    }

    // Combined score: title 70%, authors 30%
    var base_score = title_sim * 0.7 + author_sim * 0.3;

    // Boost if DOIs match exactly (case-insensitive)
    if (target.doi != null and candidate.doi != null) {
        if (std.ascii.eqlIgnoreCase(target.doi.?, candidate.doi.?)) {
            base_score = 1.0;
        }
    }

    return base_score;
}

/// Compare two entries and return a list of discrepancies.
pub fn compareEntries(allocator: std.mem.Allocator, local: *const Entry, remote: *const Entry) ![]Discrepancy {
    var discrepancies: std.ArrayList(Discrepancy) = .empty;
    errdefer {
        for (discrepancies.items) |*d| d.deinit();
        discrepancies.deinit(allocator);
    }

    // Compare titles
    if (local.title != null and remote.title != null) {
        const local_norm = try normalizeString(allocator, local.title.?);
        defer allocator.free(local_norm);
        const remote_norm = try normalizeString(allocator, remote.title.?);
        defer allocator.free(remote_norm);

        const similarity = try jaroWinklerSimilarity(allocator, local_norm, remote_norm);

        if (similarity < TITLE_MATCH_THRESHOLD) {
            const msg = try std.fmt.allocPrint(allocator, "Title significantly different (similarity: {d:.0}%)", .{similarity * 100.0});
            try discrepancies.append(allocator, .{
                .field = .title,
                .severity = .@"error",
                .local_value = try allocator.dupe(u8, local.title.?),
                .remote_value = try allocator.dupe(u8, remote.title.?),
                .message = msg,
                .allocator = allocator,
            });
        } else if (similarity < TITLE_WARNING_THRESHOLD) {
            const msg = try std.fmt.allocPrint(allocator, "Title slightly different (similarity: {d:.0}%)", .{similarity * 100.0});
            try discrepancies.append(allocator, .{
                .field = .title,
                .severity = .warning,
                .local_value = try allocator.dupe(u8, local.title.?),
                .remote_value = try allocator.dupe(u8, remote.title.?),
                .message = msg,
                .allocator = allocator,
            });
        }
    }

    // Compare years
    if (local.year != null and remote.year != null and local.year.? != remote.year.?) {
        const msg = try std.fmt.allocPrint(allocator, "Year mismatch: {d} vs {d}", .{ local.year.?, remote.year.? });
        try discrepancies.append(allocator, .{
            .field = .year,
            .severity = .@"error",
            .local_value = try std.fmt.allocPrint(allocator, "{d}", .{local.year.?}),
            .remote_value = try std.fmt.allocPrint(allocator, "{d}", .{remote.year.?}),
            .message = msg,
            .allocator = allocator,
        });
    }

    // Check for missing DOI
    if (local.doi == null and remote.doi != null) {
        try discrepancies.append(allocator, .{
            .field = .doi,
            .severity = .warning,
            .local_value = try allocator.dupe(u8, "(none)"),
            .remote_value = try allocator.dupe(u8, remote.doi.?),
            .message = try allocator.dupe(u8, "Missing DOI in local entry"),
            .allocator = allocator,
        });
    }

    // Compare author counts
    if (local.authors.len > 0 and remote.authors.len > 0 and local.authors.len != remote.authors.len) {
        const msg = try std.fmt.allocPrint(allocator, "Author count differs: {d} (local) vs {d} (remote)", .{ local.authors.len, remote.authors.len });
        try discrepancies.append(allocator, .{
            .field = .authors,
            .severity = .warning,
            .local_value = try std.fmt.allocPrint(allocator, "{d} authors", .{local.authors.len}),
            .remote_value = try std.fmt.allocPrint(allocator, "{d} authors", .{remote.authors.len}),
            .message = msg,
            .allocator = allocator,
        });
    }

    return discrepancies.toOwnedSlice(allocator);
}

/// Result of finding a best match.
pub const MatchResult = struct {
    entry: *const Entry,
    score: f64,
};

/// Find the best matching entry from a list of candidates.
pub fn findBestMatch(allocator: std.mem.Allocator, target: *const Entry, candidates: []const Entry) !?MatchResult {
    var best: ?MatchResult = null;

    for (candidates) |*candidate| {
        const score = try matchScore(allocator, target, candidate);
        if (score > 0.0) {
            if (best == null or score > best.?.score) {
                best = .{ .entry = candidate, .score = score };
            }
        }
    }

    return best;
}

test "jaroWinklerSimilarity" {
    const allocator = std.testing.allocator;

    const sim1 = try jaroWinklerSimilarity(allocator, "hello", "hello");
    try std.testing.expect(sim1 > 0.99);

    const sim2 = try jaroWinklerSimilarity(allocator, "hello", "hallo");
    try std.testing.expect(sim2 > 0.8);

    const sim3 = try jaroWinklerSimilarity(allocator, "abc", "xyz");
    try std.testing.expect(sim3 < 0.5);
}
