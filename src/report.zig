//! Validation report generation.
//!
//! Formats and outputs validation results.

const std = @import("std");
const entry_mod = @import("entry.zig");
const Entry = entry_mod.Entry;
const ApiSource = entry_mod.ApiSource;
const Discrepancy = entry_mod.Discrepancy;
const Severity = entry_mod.Severity;
const ValidationResult = entry_mod.ValidationResult;

/// Status of a validated entry.
pub const EntryStatus = union(enum) {
    ok: ApiSource,
    warning,
    @"error",
    not_found,
    failed: []const u8,
};

/// Report for a single bibliography entry.
pub const EntryReport = struct {
    entry: Entry,
    status: EntryStatus,
    validation_results: []ValidationResult,

    allocator: ?std.mem.Allocator = null,

    pub fn deinit(self: *EntryReport) void {
        if (self.allocator) |alloc| {
            var e = self.entry;
            e.deinit();
            for (self.validation_results) |*r| {
                var result = @constCast(r);
                result.deinit();
            }
            alloc.free(self.validation_results);
            if (self.status == .failed) {
                alloc.free(self.status.failed);
            }
        }
    }
};

/// Complete validation report.
pub const Report = struct {
    entries: std.ArrayList(EntryReport),
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator) Report {
        return .{
            .entries = .empty,
            .allocator = allocator,
        };
    }

    pub fn deinit(self: *Report) void {
        for (self.entries.items) |*e| e.deinit();
        self.entries.deinit(self.allocator);
    }

    pub fn add(self: *Report, report: EntryReport) !void {
        try self.entries.append(self.allocator, report);
    }

    pub fn countOk(self: *const Report) usize {
        var count: usize = 0;
        for (self.entries.items) |e| {
            if (e.status == .ok) count += 1;
        }
        return count;
    }

    pub fn countWarnings(self: *const Report) usize {
        var count: usize = 0;
        for (self.entries.items) |e| {
            if (e.status == .warning) count += 1;
        }
        return count;
    }

    pub fn countErrors(self: *const Report) usize {
        var count: usize = 0;
        for (self.entries.items) |e| {
            if (e.status == .@"error") count += 1;
        }
        return count;
    }

    pub fn countNotFound(self: *const Report) usize {
        var count: usize = 0;
        for (self.entries.items) |e| {
            if (e.status == .not_found) count += 1;
        }
        return count;
    }

    pub fn countFailed(self: *const Report) usize {
        var count: usize = 0;
        for (self.entries.items) |e| {
            if (e.status == .failed) count += 1;
        }
        return count;
    }

    /// Print the report to stdout.
    pub fn print(self: *const Report, writer: anytype, use_color: bool) !void {
        try writer.writeAll("\n");
        try printStyled(writer, "bibval Report", use_color, .bold);
        try writer.writeAll("\n");
        try writer.writeAll("==================================================\n\n");

        const total = self.entries.items.len;
        const ok = self.countOk();
        const warnings = self.countWarnings();
        const errors = self.countErrors();
        const not_found = self.countNotFound();
        const failed = self.countFailed();

        try writer.print("Processed: {d} entries\n", .{total});
        try writer.writeAll("  ");
        try printColored(writer, ok, use_color, .green);
        try writer.writeAll(" validated, ");
        try printColored(writer, warnings, use_color, .yellow);
        try writer.writeAll(" warnings, ");
        try printColored(writer, errors, use_color, .red);
        try writer.writeAll(" errors, ");
        try printColored(writer, failed, use_color, .red);
        try writer.writeAll(" failed, ");
        try printColored(writer, not_found, use_color, .dim);
        try writer.writeAll(" not found\n\n");

        // Print errors first
        try self.printSection(writer, .@"error", "ERRORS", use_color, .red);
        try self.printSection(writer, .failed, "FAILED", use_color, .red);
        try self.printSection(writer, .warning, "WARNINGS", use_color, .yellow);
        try self.printNotFoundSection(writer, use_color);
        try self.printOkSection(writer, use_color);

        try writer.writeAll("\n");
    }

    fn printSection(self: *const Report, writer: anytype, status_type: std.meta.Tag(EntryStatus), title: []const u8, use_color: bool, color: Color) !void {
        var matching: std.ArrayList(*const EntryReport) = .empty;
        defer matching.deinit(self.allocator);

        for (self.entries.items) |*e| {
            if (std.meta.activeTag(e.status) == status_type) {
                try matching.append(self.allocator, e);
            }
        }

        if (matching.items.len == 0) return;

        try printStyled(writer, title, use_color, .bold);
        try writer.print(" ({d})\n", .{matching.items.len});

        for (matching.items) |entry_report| {
            if (status_type == .failed) {
                try writer.print("  [{s}] {s}\n", .{ shortId(entry_report.entry.key), entry_report.status.failed });
            } else {
                try printEntryReport(writer, entry_report, use_color);
            }
        }
        try writer.writeAll("\n");
        _ = color;
    }

    fn printNotFoundSection(self: *const Report, writer: anytype, use_color: bool) !void {
        var matching: std.ArrayList(*const EntryReport) = .empty;
        defer matching.deinit(self.allocator);

        for (self.entries.items) |*e| {
            if (e.status == .not_found) {
                try matching.append(self.allocator, e);
            }
        }

        if (matching.items.len == 0) return;

        try printStyled(writer, "NOT FOUND", use_color, .dim);
        try writer.print(" ({d})\n", .{matching.items.len});

        for (matching.items) |entry_report| {
            const title = entry_report.entry.title orelse "(no title)";
            try writer.print("  [{s}] {s}\n", .{ shortId(entry_report.entry.key), truncate(title, 60) });
        }
        try writer.writeAll("\n");
    }

    fn printOkSection(self: *const Report, writer: anytype, use_color: bool) !void {
        var matching: std.ArrayList(*const EntryReport) = .empty;
        defer matching.deinit(self.allocator);

        for (self.entries.items) |*e| {
            if (e.status == .ok) {
                try matching.append(self.allocator, e);
            }
        }

        if (matching.items.len == 0) return;

        try printColored(writer, "OK", use_color, .green);
        try writer.print(" ({d})\n", .{matching.items.len});

        const max_display: usize = 5;
        for (matching.items[0..@min(max_display, matching.items.len)]) |entry_report| {
            const source = entry_report.status.ok;
            try writer.print("  [{s}] Validated against ", .{shortId(entry_report.entry.key)});
            try printColored(writer, source.name(), use_color, .green);
            try writer.writeAll("\n");
        }

        if (matching.items.len > max_display) {
            try writer.print("  ... {d} more...\n", .{matching.items.len - max_display});
        }
    }

    fn printEntryReport(writer: anytype, entry_report: *const EntryReport, use_color: bool) !void {
        const key = shortId(entry_report.entry.key);

        for (entry_report.validation_results) |result| {
            for (result.discrepancies) |discrepancy| {
                try printDiscrepancy(writer, key, &discrepancy, result.source, use_color);
            }
        }
    }

    fn printDiscrepancy(writer: anytype, key: []const u8, discrepancy: *const Discrepancy, source: ApiSource, use_color: bool) !void {
        try writer.print("  [{s}] ", .{key});

        switch (discrepancy.severity) {
            .@"error" => try printColored(writer, "ERROR", use_color, .red),
            .warning => try printColored(writer, "WARN", use_color, .yellow),
            .info => try printColored(writer, "INFO", use_color, .blue),
        }

        try writer.print(" {s} (via {s})\n", .{ discrepancy.message, source.name() });

        if (discrepancy.severity.order() >= Severity.warning.order()) {
            try writer.print("       Local:  {s}\n", .{truncate(discrepancy.local_value, 60)});
            try writer.print("       Remote: {s}\n", .{truncate(discrepancy.remote_value, 60)});
        }
    }
};

const Color = enum { red, green, yellow, blue, dim, bold };

fn printColored(writer: anytype, text: anytype, use_color: bool, color: Color) !void {
    if (use_color) {
        const code: []const u8 = switch (color) {
            .red => "\x1b[31m",
            .green => "\x1b[32m",
            .yellow => "\x1b[33m",
            .blue => "\x1b[34m",
            .dim => "\x1b[2m",
            .bold => "\x1b[1m",
        };
        try writer.writeAll(code);
    }

    switch (@TypeOf(text)) {
        []const u8 => try writer.writeAll(text),
        usize => try writer.print("{d}", .{text}),
        else => try writer.print("{any}", .{text}),
    }

    if (use_color) {
        try writer.writeAll("\x1b[0m");
    }
}

fn printStyled(writer: anytype, text: []const u8, use_color: bool, style: Color) !void {
    try printColored(writer, text, use_color, style);
}

fn shortId(id: []const u8) []const u8 {
    return if (id.len > 16) id[0..16] else id;
}

fn truncate(s: []const u8, max_len: usize) []const u8 {
    return if (s.len <= max_len) s else s[0..max_len];
}
