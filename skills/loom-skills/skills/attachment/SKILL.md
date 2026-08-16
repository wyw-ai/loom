---
name: attachment
description: Loom attachment and artifact operations — upload, download, publish, read, and reference files in messages and tasks.
---

# Attachment & Artifact Operations

Move files and text in and out of Loom channels and threads. Artifacts are
named blobs stored on the server, referenced by `art_…` ids or
`artifact://…` URIs. Attachments are the same artifacts when linked to
messages or tasks.

## When to use

- You need to upload a file to a channel or thread.
- You need to publish a text report as an artifact.
- You need to read a large artifact progressively (pagination).
- You need to attach a file to a message or task.
- You need to download a binary attachment.

## Scenario Routing

| Scenario | Read |
| --- | --- |
| You need to upload/download files, or understand target scoping. | `references/upload-download.md` |
| You need to publish text/file artifacts, read large artifacts with pagination, or use `artifact://` URIs. | `references/publish-read.md` |
| You need common patterns: report publishing, Windows multiline messages, paginated reading loops. | `references/common-patterns.md` |

## Command Map

| Intent | Command |
| --- | --- |
| Upload a binary file | `loom attachment upload --target "#<channel>" --path ./file.pdf` |
| Download a full attachment | `loom attachment download --id art_xxx --output ./out.pdf` |
| Peek a byte range | `loom attachment view --id art_xxx --offset 0 --max-bytes 1048576` |
| Read text body to stdout | `loom artifact read art_xxx` |
| Publish inline text | `loom artifact publish --name plan.md --text "# Plan …"` |
| Publish from a file | `loom artifact publish --name notes.md --file ./notes.md` |
| Publish from stdin | `cat ./doc.md \| loom artifact publish --name doc.md` |
| Get metadata only | `loom artifact get art_xxx` |
| Get metadata by URI | `loom artifact get artifact://chan_xxx/plan.md` |

## Attachment Lifecycle

```
upload/publish → get artifact_id → reference in message/task → download/read
```

1. **Upload or publish** the file/text to get an `art_…` id.
2. **Reference** the artifact id in a message (`--attachment-id`) or task.
3. **Download or read** the artifact content when needed.

## Core Operations

### Upload a file

```bash
loom attachment upload --target "#general" --path ./report.pdf
```

Returns: `Attachment ID: art_abc123…`. Use `-J` for JSON output when scripting.

### Reference in a message

```bash
loom message send --target "#general" --text "Report attached" \
  --attachment-id art_abc123
```

### Download an attachment

```bash
loom attachment download --id art_abc123 --output ./downloaded.pdf
```

### Publish text as an artifact

```bash
loom artifact publish --name "report.md" --text "# Report content"
# or from a file:
loom artifact publish --name "report.md" --file ./report.md
# or from stdin:
cat ./report.md | loom artifact publish --name "report.md"
```

### Read a large artifact (paginated)

```bash
loom artifact read art_abc123 --offset 0 --max-bytes 4096
# → returns content + next_offset
loom artifact read art_abc123 --offset 4096 --max-bytes 4096
# → continue until next_offset is null
```

### Get metadata

```bash
loom artifact get art_abc123
# or by URI:
loom artifact get "artifact://chan_xxx/report.md"
```

## Key Distinctions

| Concept | Description |
| --- | --- |
| **Artifact** | A named blob on the server. Identified by `art_…` id or `artifact://…` URI. |
| **Attachment** | An artifact linked to a message or task via `--attachment-id`. |
| **Upload** | `attachment upload` — sends file bytes, requires `--target`. |
| **Publish** | `artifact publish` — sends text/file content, `--in` is optional. |
| **Read** | `artifact read` — decodes UTF-8 text to stdout. Text-only. |
| **Download** | `attachment download` — writes raw bytes to a file. Binary-safe. |

## Pitfalls

1. **`attachment upload` requires `--target`.** Unlike `artifact publish`
   (optional `--in`), upload always needs a scope.
2. **`artifact read` is text-only.** It decodes the body as UTF-8. For binary
   files use `attachment download` or `attachment view`.
3. **`view` truncates at `--max-bytes`.** If stderr shows a next offset,
   switch to `download` or raise `--max-bytes`.
4. **Publish body sources are mutually exclusive.** Pick exactly one of
   `--text`, `--file`, or stdin.
5. **Artifacts without `--in` are still accessible by id** but will not
   appear in scope-filtered listings.
