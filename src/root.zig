//! bibval - Citation validator for BibTeX files
//!
//! Validates bibliographic entries against academic databases including
//! CrossRef, DBLP, arXiv, Semantic Scholar, and OpenAlex.

pub const bibtex = @import("bibtex.zig");
pub const entry = @import("entry.zig");
pub const matcher = @import("matcher.zig");
pub const http = @import("http.zig");
pub const cache = @import("cache.zig");
pub const report = @import("report.zig");
pub const validators = @import("validators.zig");

pub const Entry = entry.Entry;
pub const Discrepancy = entry.Discrepancy;
pub const ValidationResult = entry.ValidationResult;
pub const ApiSource = entry.ApiSource;
pub const Severity = entry.Severity;

test {
    @import("std").testing.refAllDecls(@This());
}
