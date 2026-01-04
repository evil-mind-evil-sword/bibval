//! bibval - Citation validator for BibTeX files
//!
//! Validates bibliographic entries against academic databases including
//! CrossRef, DBLP, Semantic Scholar, and OpenAlex.

const std = @import("std");
const bibval = @import("bibval");

const Entry = bibval.Entry;
const ApiSource = bibval.ApiSource;
const Severity = bibval.Severity;
const ValidationResult = bibval.ValidationResult;
const Discrepancy = bibval.Discrepancy;
const EntryReport = bibval.report.EntryReport;
const EntryStatus = bibval.report.EntryStatus;
const Report = bibval.report.Report;

const Args = struct {
    files: []const []const u8 = &.{},
    no_crossref: bool = false,
    no_dblp: bool = false,
    no_semantic: bool = false,
    no_openalex: bool = false,
    no_cache: bool = false,
    strict: bool = false,
    verbose: bool = false,
    json: bool = false,
    keys: []const []const u8 = &.{},
    help: bool = false,
    version: bool = false,

    allocator: std.mem.Allocator,
    files_list: std.ArrayList([]const u8),
    keys_list: std.ArrayList([]const u8),

    fn init(allocator: std.mem.Allocator) Args {
        return .{
            .allocator = allocator,
            .files_list = .empty,
            .keys_list = .empty,
        };
    }

    fn deinit(self: *Args, allocator: std.mem.Allocator) void {
        self.files_list.deinit(allocator);
        self.keys_list.deinit(allocator);
    }

    fn finalize(self: *Args) void {
        self.files = self.files_list.items;
        self.keys = self.keys_list.items;
    }
};

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    var args = try parseArgs(allocator);
    defer args.deinit(allocator);

    if (args.help) {
        printUsage();
        return;
    }

    var stdout_buf: [4096]u8 = undefined;
    var stdout_writer = std.fs.File.stdout().writer(&stdout_buf);
    const stdout = &stdout_writer.interface;
    defer stdout.flush() catch {};
    const use_color = std.fs.File.stdout().supportsAnsiEscapeCodes();

    if (args.version) {
        try stdout.writeAll("bibval 26.1.4\n");
        return;
    }

    if (args.files.len == 0) {
        std.debug.print("Error: No input files specified\n", .{});
        printUsage();
        std.process.exit(1);
    }

    // Parse all input files
    var all_entries: std.ArrayList(Entry) = .empty;
    defer {
        for (all_entries.items) |*e| e.deinit();
        all_entries.deinit(allocator);
    }

    for (args.files) |file_path| {
        // Check file exists
        std.fs.cwd().access(file_path, .{}) catch {
            std.debug.print("Error: File not found: {s}\n", .{file_path});
            std.process.exit(1);
        };

        if (!args.json) {
            try stdout.print("Parsing {s}...\n", .{file_path});
        }

        const entries = bibval.bibtex.parseFile(allocator, file_path) catch |err| {
            std.debug.print("Error: Failed to parse {s}: {s}\n", .{ file_path, @errorName(err) });
            std.process.exit(1);
        };

        if (!args.json) {
            try stdout.print("  Found {d} entries\n", .{entries.len});
        }

        for (entries) |e| {
            try all_entries.append(allocator, e);
        }
        allocator.free(entries);
    }

    if (all_entries.items.len == 0) {
        if (!args.json) {
            try stdout.writeAll("No entries found to validate.\n");
        }
        return;
    }

    // Apply key filtering if requested
    if (args.keys.len > 0) {
        var i: usize = 0;
        while (i < all_entries.items.len) {
            var found = false;
            for (args.keys) |key| {
                if (std.mem.eql(u8, all_entries.items[i].key, key)) {
                    found = true;
                    break;
                }
            }
            if (!found) {
                var removed = all_entries.orderedRemove(i);
                removed.deinit();
            } else {
                i += 1;
            }
        }

        if (all_entries.items.len == 0) {
            if (!args.json) {
                try stdout.writeAll("No entries matched the provided keys.\n");
            }
            return;
        }
    }

    if (!args.json) {
        try stdout.writeAll("\n");
        try stdout.print("Validating {d} entries...\n\n", .{all_entries.items.len});
    }

    // Initialize cache
    var response_cache = try bibval.cache.Cache.init(allocator, !args.no_cache);
    defer response_cache.deinit();

    // Initialize validators
    var crossref = if (!args.no_crossref) bibval.validators.CrossRef.init(allocator, &response_cache) else null;
    var dblp = if (!args.no_dblp) bibval.validators.Dblp.init(allocator) else null;
    var semantic = if (!args.no_semantic) bibval.validators.SemanticScholar.init(allocator) else null;
    var openalex = if (!args.no_openalex) bibval.validators.OpenAlex.init(allocator) else null;

    // Validate entries
    var report = Report.init(allocator);
    defer report.deinit();

    for (all_entries.items) |*local_entry| {
        const entry_report = try validateEntry(allocator, local_entry, &crossref, &dblp, &semantic, &openalex, args.verbose);
        try report.add(entry_report);
    }

    // Output report
    if (args.json) {
        try printJsonReport(allocator, stdout, &report);
    } else {
        try report.print(stdout, use_color);
    }

    // Determine exit code
    if (args.strict and (report.countErrors() > 0 or report.countWarnings() > 0)) {
        std.process.exit(1);
    } else if (report.countErrors() > 0) {
        std.process.exit(1);
    }
}

fn validateEntry(
    allocator: std.mem.Allocator,
    local_entry: *const Entry,
    crossref: *?bibval.validators.CrossRef,
    dblp: *?bibval.validators.Dblp,
    semantic: *?bibval.validators.SemanticScholar,
    openalex: *?bibval.validators.OpenAlex,
    verbose: bool,
) !EntryReport {
    var validation_results: std.ArrayList(ValidationResult) = .empty;
    defer validation_results.deinit(allocator);

    // Try DOI-based lookup first (most reliable)
    if (local_entry.doi != null and crossref.* != null) {
        if (crossref.*.?.searchByDoi(local_entry.doi.?)) |remote| {
            if (remote) |r| {
                var result = r;
                defer result.deinit();

                // Validate match
                const title_sim = try bibval.matcher.titleSimilarity(allocator, local_entry, &result);
                if (title_sim >= 0.75 and bibval.matcher.yearsCompatible(local_entry, &result)) {
                    const discrepancies = try bibval.matcher.compareEntries(allocator, local_entry, &result);
                    const confidence: f64 = if (discrepancies.len == 0) 1.0 else 0.8;

                    try validation_results.append(allocator, .{
                        .source = .crossref,
                        .matched_entry = null, // Don't copy entry
                        .confidence = confidence,
                        .discrepancies = discrepancies,
                        .allocator = allocator,
                    });
                }
            }
        } else |err| {
            if (verbose) {
                std.debug.print("  [{s}] CrossRef lookup failed: {}\n", .{ local_entry.key, err });
            }
        }
    }

    // Try title search if no DOI match
    if (validation_results.items.len == 0 and local_entry.title != null) {
        // Try DBLP
        if (dblp.* != null) {
            if (dblp.*.?.searchByTitle(local_entry.title.?)) |results| {
                defer {
                    for (results) |*r| {
                        var result = @constCast(r);
                        result.deinit();
                    }
                    allocator.free(results);
                }

                if (try bibval.matcher.findBestMatch(allocator, local_entry, results)) |match| {
                    const discrepancies = try bibval.matcher.compareEntries(allocator, local_entry, match.entry);
                    try validation_results.append(allocator, .{
                        .source = .dblp,
                        .matched_entry = null,
                        .confidence = match.score,
                        .discrepancies = discrepancies,
                        .allocator = allocator,
                    });
                }
            } else |err| {
                if (verbose) {
                    std.debug.print("  [{s}] DBLP lookup failed: {}\n", .{ local_entry.key, err });
                }
            }
        }

        // Try Semantic Scholar
        if (semantic.* != null) {
            if (semantic.*.?.searchByTitle(local_entry.title.?)) |results| {
                defer {
                    for (results) |*r| {
                        var result = @constCast(r);
                        result.deinit();
                    }
                    allocator.free(results);
                }

                if (try bibval.matcher.findBestMatch(allocator, local_entry, results)) |match| {
                    const discrepancies = try bibval.matcher.compareEntries(allocator, local_entry, match.entry);
                    try validation_results.append(allocator, .{
                        .source = .semantic_scholar,
                        .matched_entry = null,
                        .confidence = match.score,
                        .discrepancies = discrepancies,
                        .allocator = allocator,
                    });
                }
            } else |err| {
                if (verbose) {
                    std.debug.print("  [{s}] Semantic Scholar lookup failed: {}\n", .{ local_entry.key, err });
                }
            }
        }

        // Try OpenAlex
        if (openalex.* != null) {
            if (openalex.*.?.searchByTitle(local_entry.title.?)) |results| {
                defer {
                    for (results) |*r| {
                        var result = @constCast(r);
                        result.deinit();
                    }
                    allocator.free(results);
                }

                if (try bibval.matcher.findBestMatch(allocator, local_entry, results)) |match| {
                    const discrepancies = try bibval.matcher.compareEntries(allocator, local_entry, match.entry);
                    try validation_results.append(allocator, .{
                        .source = .openalex,
                        .matched_entry = null,
                        .confidence = match.score,
                        .discrepancies = discrepancies,
                        .allocator = allocator,
                    });
                }
            } else |err| {
                if (verbose) {
                    std.debug.print("  [{s}] OpenAlex lookup failed: {}\n", .{ local_entry.key, err });
                }
            }
        }
    }

    // Determine status
    const status = determineStatus(&validation_results);

    // Clone entry for report
    var entry_copy = Entry{
        .key = try allocator.dupe(u8, local_entry.key),
        .entry_type = try allocator.dupe(u8, local_entry.entry_type),
        .title = if (local_entry.title) |t| try allocator.dupe(u8, t) else null,
        .year = local_entry.year,
        .venue = if (local_entry.venue) |v| try allocator.dupe(u8, v) else null,
        .doi = if (local_entry.doi) |d| try allocator.dupe(u8, d) else null,
        .arxiv_id = if (local_entry.arxiv_id) |a| try allocator.dupe(u8, a) else null,
        .url = if (local_entry.url) |u| try allocator.dupe(u8, u) else null,
        .allocator = allocator,
    };

    // Clone authors
    if (local_entry.authors.len > 0) {
        var authors = try allocator.alloc([]const u8, local_entry.authors.len);
        for (local_entry.authors, 0..) |a, i| {
            authors[i] = try allocator.dupe(u8, a);
        }
        entry_copy.authors = authors;
    }

    return EntryReport{
        .entry = entry_copy,
        .status = status,
        .validation_results = try validation_results.toOwnedSlice(allocator),
        .allocator = allocator,
    };
}

fn determineStatus(results: *const std.ArrayList(ValidationResult)) EntryStatus {
    if (results.items.len == 0) {
        return .not_found;
    }

    var has_errors = false;
    var has_warnings = false;
    var best_source: ApiSource = .crossref;
    var best_confidence: f64 = 0;

    for (results.items) |result| {
        if (result.confidence > best_confidence) {
            best_confidence = result.confidence;
            best_source = result.source;
        }

        for (result.discrepancies) |d| {
            if (d.severity == .@"error") has_errors = true;
            if (d.severity == .warning) has_warnings = true;
        }
    }

    if (has_errors) return .@"error";
    if (has_warnings) return .warning;
    return .{ .ok = best_source };
}

fn printJsonReport(allocator: std.mem.Allocator, writer: anytype, report: *const Report) !void {
    try writer.writeAll("{\"entries\":[");

    var first = true;
    for (report.entries.items) |entry_report| {
        if (!first) try writer.writeAll(",");
        first = false;

        try writer.writeAll("{\"key\":");
        try writeJsonString(writer, entry_report.entry.key);
        try writer.writeAll(",\"title\":");
        if (entry_report.entry.title) |t| {
            try writeJsonString(writer, t);
        } else {
            try writer.writeAll("null");
        }
        try writer.writeAll(",\"status\":\"");
        switch (entry_report.status) {
            .ok => |source| try writer.print("ok:{s}", .{source.name()}),
            .warning => try writer.writeAll("warning"),
            .@"error" => try writer.writeAll("error"),
            .not_found => try writer.writeAll("not_found"),
            .failed => |msg| try writer.print("failed:{s}", .{msg}),
        }
        try writer.writeAll("\",\"discrepancies\":[");

        var disc_first = true;
        for (entry_report.validation_results) |result| {
            for (result.discrepancies) |d| {
                if (!disc_first) try writer.writeAll(",");
                disc_first = false;

                try writer.writeAll("{\"field\":\"");
                try writer.writeAll(d.field.name());
                try writer.writeAll("\",\"severity\":\"");
                try writer.writeAll(d.severity.name());
                try writer.writeAll("\",\"message\":");
                try writeJsonString(writer, d.message);
                try writer.writeAll("}");
            }
        }
        try writer.writeAll("]}");
    }

    try writer.writeAll("],\"summary\":{");
    try writer.print("\"total\":{d},\"ok\":{d},\"warnings\":{d},\"errors\":{d},\"not_found\":{d}", .{
        report.entries.items.len,
        report.countOk(),
        report.countWarnings(),
        report.countErrors(),
        report.countNotFound(),
    });
    try writer.writeAll("}}\n");
    _ = allocator;
}

fn writeJsonString(writer: anytype, s: []const u8) !void {
    try writer.writeByte('"');
    for (s) |c| {
        switch (c) {
            '"' => try writer.writeAll("\\\""),
            '\\' => try writer.writeAll("\\\\"),
            '\n' => try writer.writeAll("\\n"),
            '\r' => try writer.writeAll("\\r"),
            '\t' => try writer.writeAll("\\t"),
            0x08 => try writer.writeAll("\\b"), // backspace
            0x0C => try writer.writeAll("\\f"), // form feed
            else => {
                if (c < 0x20) {
                    try writer.print("\\u{x:0>4}", .{c});
                } else {
                    try writer.writeByte(c);
                }
            },
        }
    }
    try writer.writeByte('"');
}

fn parseArgs(allocator: std.mem.Allocator) !Args {
    var args = Args.init(allocator);
    errdefer args.deinit(allocator);

    var arg_iter = try std.process.argsWithAllocator(allocator);
    defer arg_iter.deinit();

    _ = arg_iter.next(); // Skip program name

    while (arg_iter.next()) |arg| {
        if (std.mem.eql(u8, arg, "--help") or std.mem.eql(u8, arg, "-h")) {
            args.help = true;
        } else if (std.mem.eql(u8, arg, "--version") or std.mem.eql(u8, arg, "-V")) {
            args.version = true;
        } else if (std.mem.eql(u8, arg, "--no-crossref")) {
            args.no_crossref = true;
        } else if (std.mem.eql(u8, arg, "--no-dblp")) {
            args.no_dblp = true;
        } else if (std.mem.eql(u8, arg, "--no-semantic")) {
            args.no_semantic = true;
        } else if (std.mem.eql(u8, arg, "--no-openalex")) {
            args.no_openalex = true;
        } else if (std.mem.eql(u8, arg, "--no-cache")) {
            args.no_cache = true;
        } else if (std.mem.eql(u8, arg, "--strict") or std.mem.eql(u8, arg, "-s")) {
            args.strict = true;
        } else if (std.mem.eql(u8, arg, "--verbose") or std.mem.eql(u8, arg, "-v")) {
            args.verbose = true;
        } else if (std.mem.eql(u8, arg, "--json")) {
            args.json = true;
        } else if (std.mem.eql(u8, arg, "-k") or std.mem.eql(u8, arg, "--key")) {
            if (arg_iter.next()) |key| {
                // Handle comma-separated keys
                var iter = std.mem.splitScalar(u8, key, ',');
                while (iter.next()) |k| {
                    try args.keys_list.append(allocator, k);
                }
            }
        } else if (arg.len > 0 and arg[0] != '-') {
            try args.files_list.append(allocator, arg);
        } else if (std.mem.startsWith(u8, arg, "--")) {
            std.debug.print("Unknown option: {s}\n", .{arg});
            std.process.exit(1);
        }
    }

    args.finalize();
    return args;
}

fn printUsage() void {
    std.debug.print(
        \\bibval - Citation validator for BibTeX files
        \\
        \\Usage:
        \\  bibval [options] <file.bib> [file2.bib ...]
        \\
        \\Options:
        \\  -h, --help        Show this help
        \\  -V, --version     Show version
        \\  -s, --strict      Exit with error if any issues found
        \\  -v, --verbose     Verbose output
        \\  --json            Output JSON format
        \\  -k, --key KEY     Only validate entries with these keys (comma-separated)
        \\  --no-crossref     Disable CrossRef API
        \\  --no-dblp         Disable DBLP API
        \\  --no-semantic     Disable Semantic Scholar API
        \\  --no-openalex     Disable OpenAlex API
        \\  --no-cache        Disable response caching
        \\
        \\Example:
        \\  bibval references.bib
        \\  bibval paper.bib thesis.bib --strict
        \\  bibval refs.bib -k smith2021,jones2022 --json
        \\
    , .{});
}
