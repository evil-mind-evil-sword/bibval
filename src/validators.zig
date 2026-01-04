//! API validators for academic databases.

const std = @import("std");
const http = @import("http.zig");
const cache = @import("cache.zig");
const entry_mod = @import("entry.zig");
const Entry = entry_mod.Entry;
const ApiSource = entry_mod.ApiSource;

pub const ValidatorError = error{
    RequestFailed,
    RateLimited,
    NotFound,
    ParseError,
    OutOfMemory,
    ConnectionRefused,
    Timeout,
    InvalidUrl,
};

const USER_AGENT = "bibval/0.1.0 (https://github.com/evil-mind-evil-sword/bibval)";

pub const CrossRef = struct {
    allocator: std.mem.Allocator,
    client: http.Client,
    response_cache: *cache.Cache,

    const BASE_URL = "https://api.crossref.org/works";

    pub fn init(allocator: std.mem.Allocator, response_cache: *cache.Cache) CrossRef {
        return .{
            .allocator = allocator,
            .client = http.Client.init(allocator, USER_AGENT),
            .response_cache = response_cache,
        };
    }

    pub fn searchByDoi(self: *CrossRef, doi: []const u8) !?Entry {
        if (self.response_cache.get("crossref_doi", doi)) |cached| {
            defer self.allocator.free(cached);
            return try parseWork(self.allocator, cached);
        }

        const url = try std.fmt.allocPrint(self.allocator, "{s}/{s}", .{ BASE_URL, doi });
        defer self.allocator.free(url);

        const body = self.client.get(url) catch |err| {
            return switch (err) {
                http.HttpError.NotFound => null,
                http.HttpError.RateLimited => ValidatorError.RateLimited,
                else => ValidatorError.RequestFailed,
            };
        };
        defer self.allocator.free(body);

        self.response_cache.set("crossref_doi", doi, body) catch {};
        return try parseWork(self.allocator, body);
    }

    pub fn searchByTitle(self: *CrossRef, title: []const u8) ![]Entry {
        const encoded = try http.urlEncode(self.allocator, title);
        defer self.allocator.free(encoded);

        const url = try std.fmt.allocPrint(self.allocator, "{s}?query.title={s}&rows=5", .{ BASE_URL, encoded });
        defer self.allocator.free(url);

        const body = self.client.get(url) catch |err| {
            return switch (err) {
                http.HttpError.RateLimited => ValidatorError.RateLimited,
                else => ValidatorError.RequestFailed,
            };
        };
        defer self.allocator.free(body);

        return try parseSearchResults(self.allocator, body);
    }

    fn parseWork(allocator: std.mem.Allocator, json_body: []const u8) !?Entry {
        const parsed = std.json.parseFromSlice(std.json.Value, allocator, json_body, .{}) catch return null;
        defer parsed.deinit();

        const root = parsed.value;
        if (root != .object) return null;

        const status = root.object.get("status") orelse return null;
        if (status != .string or !std.mem.eql(u8, status.string, "ok")) return null;

        const message = root.object.get("message") orelse return null;
        if (message != .object) return null;

        return try workToEntry(allocator, message.object);
    }

    fn parseSearchResults(allocator: std.mem.Allocator, json_body: []const u8) ![]Entry {
        const parsed = std.json.parseFromSlice(std.json.Value, allocator, json_body, .{}) catch return &.{};
        defer parsed.deinit();

        const root = parsed.value;
        if (root != .object) return &.{};

        const message = root.object.get("message") orelse return &.{};
        if (message != .object) return &.{};

        const items = message.object.get("items") orelse return &.{};
        if (items != .array) return &.{};

        var entries: std.ArrayList(Entry) = .empty;
        errdefer {
            for (entries.items) |*e| e.deinit();
            entries.deinit(allocator);
        }

        for (items.array.items) |item| {
            if (item != .object) continue;
            if (workToEntry(allocator, item.object)) |e| {
                try entries.append(allocator, e);
            } else |_| {}
        }

        return entries.toOwnedSlice(allocator);
    }

    fn workToEntry(allocator: std.mem.Allocator, work: std.json.ObjectMap) !Entry {
        var result = Entry{
            .key = "",
            .entry_type = try allocator.dupe(u8, "article"),
            .allocator = allocator,
        };
        errdefer result.deinit();

        if (work.get("DOI")) |doi_val| {
            if (doi_val == .string) {
                result.doi = try allocator.dupe(u8, doi_val.string);
                result.key = try allocator.dupe(u8, doi_val.string);
            }
        }

        if (work.get("title")) |title_val| {
            if (title_val == .array and title_val.array.items.len > 0) {
                if (title_val.array.items[0] == .string) {
                    result.title = try allocator.dupe(u8, title_val.array.items[0].string);
                }
            }
        }

        if (work.get("author")) |author_val| {
            if (author_val == .array) {
                var authors: std.ArrayList([]const u8) = .empty;
                defer authors.deinit(allocator);
                for (author_val.array.items) |author| {
                    if (author != .object) continue;
                    const given = if (author.object.get("given")) |g| if (g == .string) g.string else "" else "";
                    const family = if (author.object.get("family")) |f| if (f == .string) f.string else "" else "";
                    const name = try std.fmt.allocPrint(allocator, "{s} {s}", .{ given, family });
                    defer allocator.free(name);
                    const trimmed = std.mem.trim(u8, name, " ");
                    try authors.append(allocator, try allocator.dupe(u8, trimmed));
                }
                result.authors = try authors.toOwnedSlice(allocator);
            }
        }

        if (work.get("container-title")) |venue_val| {
            if (venue_val == .array and venue_val.array.items.len > 0) {
                if (venue_val.array.items[0] == .string) {
                    result.venue = try allocator.dupe(u8, venue_val.array.items[0].string);
                }
            }
        }

        const date_fields = [_][]const u8{ "published", "published-print", "published-online" };
        for (date_fields) |field_name| {
            if (work.get(field_name)) |date_val| {
                if (date_val == .object) {
                    if (date_val.object.get("date-parts")) |parts| {
                        if (parts == .array and parts.array.items.len > 0) {
                            if (parts.array.items[0] == .array and parts.array.items[0].array.items.len > 0) {
                                if (parts.array.items[0].array.items[0] == .integer) {
                                    result.year = @intCast(parts.array.items[0].array.items[0].integer);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        if (work.get("type")) |type_val| {
            if (type_val == .string) {
                const new_type = try allocator.dupe(u8, type_val.string);
                allocator.free(result.entry_type);
                result.entry_type = new_type;
            }
        }

        return result;
    }
};

pub const Dblp = struct {
    allocator: std.mem.Allocator,
    client: http.Client,

    const BASE_URL = "https://dblp.org/search/publ/api";

    pub fn init(allocator: std.mem.Allocator) Dblp {
        return .{
            .allocator = allocator,
            .client = http.Client.init(allocator, USER_AGENT),
        };
    }

    pub fn searchByTitle(self: *Dblp, title: []const u8) ![]Entry {
        const encoded = try http.urlEncode(self.allocator, title);
        defer self.allocator.free(encoded);

        const url = try std.fmt.allocPrint(self.allocator, "{s}?q={s}&format=json&h=5", .{ BASE_URL, encoded });
        defer self.allocator.free(url);

        const body = self.client.get(url) catch |err| {
            return switch (err) {
                http.HttpError.RateLimited => ValidatorError.RateLimited,
                else => ValidatorError.RequestFailed,
            };
        };
        defer self.allocator.free(body);

        return try parseResults(self.allocator, body);
    }

    fn parseResults(allocator: std.mem.Allocator, json_body: []const u8) ![]Entry {
        const parsed = std.json.parseFromSlice(std.json.Value, allocator, json_body, .{}) catch return &.{};
        defer parsed.deinit();

        const root = parsed.value;
        if (root != .object) return &.{};

        const result_obj = root.object.get("result") orelse return &.{};
        if (result_obj != .object) return &.{};

        const hits = result_obj.object.get("hits") orelse return &.{};
        if (hits != .object) return &.{};

        const hit_array = hits.object.get("hit") orelse return &.{};
        if (hit_array != .array) return &.{};

        var entries: std.ArrayList(Entry) = .empty;
        errdefer {
            for (entries.items) |*e| e.deinit();
            entries.deinit(allocator);
        }

        for (hit_array.array.items) |hit| {
            if (hit != .object) continue;
            const info = hit.object.get("info") orelse continue;
            if (info != .object) continue;

            if (infoToEntry(allocator, info.object)) |e| {
                try entries.append(allocator, e);
            } else |_| {}
        }

        return entries.toOwnedSlice(allocator);
    }

    fn infoToEntry(allocator: std.mem.Allocator, info: std.json.ObjectMap) !Entry {
        var result = Entry{
            .key = "",
            .entry_type = try allocator.dupe(u8, "article"),
            .allocator = allocator,
        };
        errdefer result.deinit();

        if (info.get("title")) |title_val| {
            if (title_val == .string) {
                var title = title_val.string;
                if (std.mem.endsWith(u8, title, ".")) {
                    title = title[0 .. title.len - 1];
                }
                result.title = try allocator.dupe(u8, title);
            }
        }

        if (info.get("doi")) |doi_val| {
            if (doi_val == .string) {
                result.doi = try allocator.dupe(u8, doi_val.string);
            }
        }

        if (info.get("year")) |year_val| {
            if (year_val == .string) {
                result.year = std.fmt.parseInt(i32, year_val.string, 10) catch null;
            }
        }

        if (info.get("venue")) |venue_val| {
            if (venue_val == .string) {
                result.venue = try allocator.dupe(u8, venue_val.string);
            }
        }

        if (info.get("authors")) |authors_obj| {
            if (authors_obj == .object) {
                if (authors_obj.object.get("author")) |author_val| {
                    var authors: std.ArrayList([]const u8) = .empty;
                    defer authors.deinit(allocator);

                    if (author_val == .array) {
                        for (author_val.array.items) |author| {
                            if (author == .string) {
                                try authors.append(allocator, try allocator.dupe(u8, author.string));
                            } else if (author == .object) {
                                if (author.object.get("text")) |text| {
                                    if (text == .string) {
                                        try authors.append(allocator, try allocator.dupe(u8, text.string));
                                    }
                                }
                            }
                        }
                    } else if (author_val == .string) {
                        try authors.append(allocator, try allocator.dupe(u8, author_val.string));
                    } else if (author_val == .object) {
                        if (author_val.object.get("text")) |text| {
                            if (text == .string) {
                                try authors.append(allocator, try allocator.dupe(u8, text.string));
                            }
                        }
                    }

                    result.authors = try authors.toOwnedSlice(allocator);
                }
            }
        }

        if (info.get("url")) |url_val| {
            if (url_val == .string) {
                result.key = try allocator.dupe(u8, url_val.string);
            }
        }

        if (info.get("type")) |type_val| {
            if (type_val == .string) {
                const new_type = try allocator.dupe(u8, type_val.string);
                allocator.free(result.entry_type);
                result.entry_type = new_type;
            }
        }

        return result;
    }
};

pub const SemanticScholar = struct {
    allocator: std.mem.Allocator,
    client: http.Client,

    const BASE_URL = "https://api.semanticscholar.org/graph/v1";

    pub fn init(allocator: std.mem.Allocator) SemanticScholar {
        return .{
            .allocator = allocator,
            .client = http.Client.init(allocator, USER_AGENT),
        };
    }

    pub fn searchByTitle(self: *SemanticScholar, title: []const u8) ![]Entry {
        const encoded = try http.urlEncode(self.allocator, title);
        defer self.allocator.free(encoded);

        const url = try std.fmt.allocPrint(self.allocator, "{s}/paper/search?query={s}&fields=title,authors,year,venue,externalIds&limit=5", .{ BASE_URL, encoded });
        defer self.allocator.free(url);

        const body = self.client.get(url) catch |err| {
            return switch (err) {
                http.HttpError.RateLimited => ValidatorError.RateLimited,
                else => ValidatorError.RequestFailed,
            };
        };
        defer self.allocator.free(body);

        return try parseResults(self.allocator, body);
    }

    fn parseResults(allocator: std.mem.Allocator, json_body: []const u8) ![]Entry {
        const parsed = std.json.parseFromSlice(std.json.Value, allocator, json_body, .{}) catch return &.{};
        defer parsed.deinit();

        const root = parsed.value;
        if (root != .object) return &.{};

        const data = root.object.get("data") orelse return &.{};
        if (data != .array) return &.{};

        var entries: std.ArrayList(Entry) = .empty;
        errdefer {
            for (entries.items) |*e| e.deinit();
            entries.deinit(allocator);
        }

        for (data.array.items) |paper| {
            if (paper != .object) continue;
            if (paperToEntry(allocator, paper.object)) |e| {
                try entries.append(allocator, e);
            } else |_| {}
        }

        return entries.toOwnedSlice(allocator);
    }

    fn paperToEntry(allocator: std.mem.Allocator, paper: std.json.ObjectMap) !Entry {
        var result = Entry{
            .key = "",
            .entry_type = try allocator.dupe(u8, "article"),
            .allocator = allocator,
        };
        errdefer result.deinit();

        if (paper.get("paperId")) |id_val| {
            if (id_val == .string) {
                result.key = try allocator.dupe(u8, id_val.string);
            }
        }

        if (paper.get("title")) |title_val| {
            if (title_val == .string) {
                result.title = try allocator.dupe(u8, title_val.string);
            }
        }

        if (paper.get("year")) |year_val| {
            if (year_val == .integer) {
                result.year = @intCast(year_val.integer);
            }
        }

        if (paper.get("venue")) |venue_val| {
            if (venue_val == .string and venue_val.string.len > 0) {
                result.venue = try allocator.dupe(u8, venue_val.string);
            }
        }

        if (paper.get("authors")) |authors_val| {
            if (authors_val == .array) {
                var authors: std.ArrayList([]const u8) = .empty;
                defer authors.deinit(allocator);
                for (authors_val.array.items) |author| {
                    if (author != .object) continue;
                    if (author.object.get("name")) |name| {
                        if (name == .string) {
                            try authors.append(allocator, try allocator.dupe(u8, name.string));
                        }
                    }
                }
                result.authors = try authors.toOwnedSlice(allocator);
            }
        }

        if (paper.get("externalIds")) |ids| {
            if (ids == .object) {
                if (ids.object.get("DOI")) |doi| {
                    if (doi == .string) {
                        result.doi = try allocator.dupe(u8, doi.string);
                    }
                }
                if (ids.object.get("ArXiv")) |arxiv| {
                    if (arxiv == .string) {
                        result.arxiv_id = try allocator.dupe(u8, arxiv.string);
                    }
                }
            }
        }

        return result;
    }
};

pub const OpenAlex = struct {
    allocator: std.mem.Allocator,
    client: http.Client,

    const BASE_URL = "https://api.openalex.org/works";

    pub fn init(allocator: std.mem.Allocator) OpenAlex {
        return .{
            .allocator = allocator,
            .client = http.Client.init(allocator, USER_AGENT),
        };
    }

    pub fn searchByTitle(self: *OpenAlex, title: []const u8) ![]Entry {
        const encoded = try http.urlEncode(self.allocator, title);
        defer self.allocator.free(encoded);

        const url = try std.fmt.allocPrint(self.allocator, "{s}?filter=title.search:{s}&per-page=5", .{ BASE_URL, encoded });
        defer self.allocator.free(url);

        const body = self.client.get(url) catch |err| {
            return switch (err) {
                http.HttpError.RateLimited => ValidatorError.RateLimited,
                else => ValidatorError.RequestFailed,
            };
        };
        defer self.allocator.free(body);

        return try parseResults(self.allocator, body);
    }

    fn parseResults(allocator: std.mem.Allocator, json_body: []const u8) ![]Entry {
        const parsed = std.json.parseFromSlice(std.json.Value, allocator, json_body, .{}) catch return &.{};
        defer parsed.deinit();

        const root = parsed.value;
        if (root != .object) return &.{};

        const results = root.object.get("results") orelse return &.{};
        if (results != .array) return &.{};

        var entries: std.ArrayList(Entry) = .empty;
        errdefer {
            for (entries.items) |*e| e.deinit();
            entries.deinit(allocator);
        }

        for (results.array.items) |work| {
            if (work != .object) continue;
            if (workToEntry(allocator, work.object)) |e| {
                try entries.append(allocator, e);
            } else |_| {}
        }

        return entries.toOwnedSlice(allocator);
    }

    fn workToEntry(allocator: std.mem.Allocator, work: std.json.ObjectMap) !Entry {
        var result = Entry{
            .key = "",
            .entry_type = try allocator.dupe(u8, "article"),
            .allocator = allocator,
        };
        errdefer result.deinit();

        if (work.get("id")) |id_val| {
            if (id_val == .string) {
                result.key = try allocator.dupe(u8, id_val.string);
            }
        }

        if (work.get("title")) |title_val| {
            if (title_val == .string) {
                result.title = try allocator.dupe(u8, title_val.string);
            }
        }

        if (work.get("doi")) |doi_val| {
            if (doi_val == .string) {
                var doi = doi_val.string;
                if (std.mem.startsWith(u8, doi, "https://doi.org/")) {
                    doi = doi[16..];
                }
                result.doi = try allocator.dupe(u8, doi);
            }
        }

        if (work.get("publication_year")) |year_val| {
            if (year_val == .integer) {
                result.year = @intCast(year_val.integer);
            }
        }

        if (work.get("authorships")) |authorships| {
            if (authorships == .array) {
                var authors: std.ArrayList([]const u8) = .empty;
                defer authors.deinit(allocator);
                for (authorships.array.items) |authorship| {
                    if (authorship != .object) continue;
                    if (authorship.object.get("author")) |author| {
                        if (author == .object) {
                            if (author.object.get("display_name")) |name| {
                                if (name == .string) {
                                    try authors.append(allocator, try allocator.dupe(u8, name.string));
                                }
                            }
                        }
                    }
                }
                result.authors = try authors.toOwnedSlice(allocator);
            }
        }

        return result;
    }
};
