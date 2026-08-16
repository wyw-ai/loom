# Publish & Read

Publish text and file artifacts, read them back with pagination, and use
`artifact://` URIs for reference.

## Publish Text Artifacts

`artifact publish` stores text or file content as a named artifact on the
server.

### Three input modes (mutually exclusive)

```bash
# Mode 1: Inline text
loom artifact publish --name plan.md --text "# Plan"

# Mode 2: From a file
loom artifact publish --name notes.md --file ./notes.md

# Mode 3: From stdin (pipe)
echo "body" | loom artifact publish --name doc.md
```

### Parameters

| Parameter | Required | Default | Description |
| --- | --- | --- | --- |
| `--name` | Yes | — | Artifact file name |
| `--text` | One of three | — | Inline text content |
| `--file` | One of three | — | Local file path to read |
| *(stdin)* | One of three | — | Read from standard input |
| `--media-type` | No | `text/markdown` | MIME type override |
| `--in <id>` | No | — | Associate with a scope. Add `--channel` to treat as channel id |

### Output

```
<art_id>\t<media_type>\t<size> bytes\t<uri>
```

Example:
```
art_abc123    text/markdown    4096 bytes    artifact://chan_xxx/plan.md
```

### Scope association

- **With `--in <scope_id>`**: The artifact is associated with a thread scope.
  Add `--channel` to treat the id as a channel id.
- **Without `--in`**: The artifact is still accessible by id but will not
  appear in scope-filtered listings.

## Read Text Body

`artifact read` decodes the artifact body as UTF-8 and prints to stdout.

```bash
loom artifact read art_abc123
```

### Pagination for large artifacts

| Parameter | Default | Description |
| --- | --- | --- |
| `--offset` | 0 | Start byte offset |
| `--max-bytes` | 64 KiB | Maximum bytes to read in one request |

For large artifacts, read progressively:

```bash
# First page
loom artifact read art_abc123 --offset 0 --max-bytes 4096
# Output includes content + next_offset field

# Next page
loom artifact read art_abc123 --offset 4096 --max-bytes 4096

# Continue until next_offset is null/absent
```

### JSON output for pagination control

Use `--json` (`-J`) to get structured output with explicit `next_offset`:

```bash
loom artifact read art_abc123 --offset 0 --max-bytes 4096 -J
# → { "content": "...", "offset": 0, "nextOffset": 4096, "truncated": true }
```

When `nextOffset` is `null` or absent, the artifact has been fully read.

### Large file strategy

| Artifact size | Strategy |
| --- | --- |
| < 64 KiB | Single `artifact read` (default) |
| 64 KiB – 1 MiB | Paginated `artifact read` with `--max-bytes 65536` |
| > 1 MiB | Use `attachment download` for full payload, or paginated read with larger `--max-bytes` |

## artifact:// URI Format

Artifacts can be referenced by id or by URI:

| Form | Example |
| --- | --- |
| **id** | `art_abc123` |
| **URI** | `artifact://chan_xxx/report.md` |

The URI form encodes the scope and name:
```
artifact://<scope_id>/<artifact_name>
```

Both forms work with `artifact get` and `artifact read`:

```bash
loom artifact get art_abc123
loom artifact get "artifact://chan_xxx/report.md"

loom artifact read art_abc123
loom artifact read "artifact://chan_xxx/report.md"
```

## Get Metadata

`artifact get` returns metadata without the body:

```bash
loom artifact get art_abc123
```

Returns: id, name, media type, size, URI, and scope association.

Useful for checking artifact size before deciding whether to read inline or
download to a file.

## Publish vs Upload

| Aspect | `artifact publish` | `attachment upload` |
| --- | --- | --- |
| **Input** | Text, file, or stdin | File only |
| **`--target` required** | No (`--in` is optional) | Yes |
| **Default media type** | `text/markdown` | `application/octet-stream` |
| **Typical use** | Text reports, markdown docs | Binary files, images, PDFs |
| **stdin support** | Yes | No |

Use `publish` for text content; use `upload` for binary files.
