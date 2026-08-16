# Attachments And Artifacts

Move files and text in and out of Loom channels and threads. Artifacts are
named blobs stored on the server, referenced by `art_…` ids or
`artifact://…` URIs. Attachments are the same artifacts when linked to
messages or tasks.

## Command Map

| Intent | Command |
| --- | --- |
| Upload a binary file | `loom attachment upload --target "#<channel>" --path ./file.pdf` |
| Download a full attachment | `loom attachment download --id art_xxx --output ./out.pdf` |
| Peek a byte range | `loom attachment view --id art_xxx --output ./out.bin --offset 0 --max-bytes 1048576` |
| Read text body to stdout | `loom artifact read art_xxx` |
| Publish inline text | `loom artifact publish --name plan.md --text "# Plan …"` |
| Publish from a file | `loom artifact publish --name notes.md --file ./notes.md` |
| Publish from stdin | `cat ./doc.md \| loom artifact publish --name doc.md` |
| Get metadata only | `loom artifact get art_xxx` |
| Get metadata by URI | `loom artifact get artifact://chan_xxx/plan.md` |

## Upload

```bash
loom attachment upload --target "#general" --path ./report.pdf
```

- `--target` — channel (`#chan_…`), thread (`#chan_…:msg_…`), or DM
  (`dm:@actor_…`). Required.
- `--mime-type` — override the default `application/octet-stream`.

Output: `Attachment ID: art_abc123…`. Add `--json` (`-J`) for structured
output when scripting.

## Download

### Full download (auto-chunks until complete)

```bash
loom attachment download --id art_abc123 --output ./downloaded.pdf
```

`--chunk-bytes` controls the internal loop size (default 1 MiB). The CLI
always fetches the entire file regardless of size.

### Single-read view (one request, may truncate)

```bash
loom attachment view --id art_abc123 --output ./out.bin
```

- `--offset` — start byte (default 0).
- `--max-bytes` — request cap (default 10 MiB). If truncated, the next
  offset is printed to stderr.

Use `view` for a byte-range peek; use `download` for the full payload.

## Publish Text Artifacts

```bash
loom artifact publish --name plan.md --text "# Plan"
loom artifact publish --name notes.md --file ./notes.md
echo "body" | loom artifact publish --name doc.md
```

- `--media-type` — defaults to `text/markdown`.
- `--in <id>` — associate with a thread scope. Add `--channel` to treat
  `--in` as a channel id.
- `--text`, `--file`, and stdin are mutually exclusive; pick one.

Output: `<art_id>\t<media_type>\t<size> bytes\t<uri>`.

## Read Text Body

```bash
loom artifact read art_abc123
```

Prints the decoded UTF-8 body to stdout. Use `--offset` / `--max-bytes`
for progressive reads of large artifacts (defaults 0 / 64 KiB).

## Attach To Messages And Tasks

Message commands accept repeatable `--attachment-id`:

```bash
loom message send --target "#general" --text "Report attached" \
  --attachment-id art_abc123

loom message ask @actor_agent_fe --target "#general" --text "Review this" \
  --attachment-id art_abc123
```

Task commands:

```bash
loom task update <task_id> --artifact-id art_abc123
loom task complete <task_id> --result "Done" --artifact-id art_abc123
```

## Upload-Then-Send Pattern

```bash
ART_ID=$(loom attachment upload -J --target "#general" --path ./report.pdf \
  | python -c "import sys,json; print(json.load(sys.stdin)['artifact']['id'])")
loom message send --target "#general" --text "Report attached" \
  --attachment-id "$ART_ID"
```

## Long-Message Truncation (Critical)

When a human sends a long message (≥ 4000 chars) from the GUI, the body
is **not** delivered inline. Instead the GUI:

1. Publishes the full text as a `message-<timestamp>.txt` text/plain
   artifact.
2. Replaces the message body with the first **500 chars** plus:
   ```
   [... full text attached as .txt ...]
   ```
3. Appends a `[附件]` (attachments) block listing every attachment:
   ```
   [附件]
   - message-1786550723083.txt (text/plain, 6.9 KB) [artifact: art_f5fcf973035f]
   - 讨论记录.txt (text/plain, 131 KB) [artifact: art_03f415a21d1c]
   ```

A second, independent truncation happens in the agent runtime: the
triggering message body is capped at **4000 chars** and context messages
at **500 chars**. When this fires the prompt shows:
```
(message body truncated: showing 4000 of 12345 chars; run `loom --json message get <msgId>` for the full text)
```

### What you must do

**Whenever you see `[... full text attached as .txt ...]` or a
`[附件]` block, the message you received is incomplete.** You MUST fetch
the full content before acting:

```bash
# Extract artifact ids from the [附件] block, then read each one:
loom artifact read art_f5fcf973035f              # text → stdout
loom artifact read art_f5fcf973035f --max-bytes 1048576   # large text, progressive

# Or download to a file for binary / very large attachments:
loom attachment download --id art_03f415a21d1c --output ./讨论记录.txt
```

### Recognising the pattern

Treat any of these as a signal that full content lives in an artifact:

| Signal | Meaning |
| --- | --- |
| `[... full text attached as .txt ...]` | The original message body was converted to a `message-*.txt` artifact. |
| `[附件]` block with `[artifact: art_…]` | One or more artifacts are attached; parse the `art_…` ids. |
| `message-<digits>.txt` attachment name | The full text of a long user message. Always read it. |
| `(message body truncated: showing N of M chars…)` | Runtime truncation; use `loom --json message get <msgId>` for the raw body. |

### Parsing the `[附件]` block

The block uses a stable line format:

```
- <name> (<mediaType>, <humanSize>) [artifact: <art_id>]
```

Extract the `art_…` id from each line. Names containing CJK characters
or spaces are preserved as-is. The `mediaType` tells you whether to use
`artifact read` (text/*, application/json, etc.) or `attachment download`
(binary: images, PDFs, archives).

### Decision flow

1. Message contains `[... full text attached as .txt ...]`?
   → Read **every** `message-*.txt` artifact before responding.
2. Message contains `[附件]` with non-`message-*` artifacts?
   → Read/download the relevant artifacts for your task.
3. Message body truncated by runtime (`showing N of M chars`)?
   → Run `loom --json message get <msgId>`; if the body still references
     artifacts, go to step 1 or 2.

Failing to fetch the full text means you are working from a 500-char
prefix of a message that may be thousands of characters long.

## Pitfalls

1. **`attachment upload` requires `--target`.** Unlike `artifact publish`
   (optional `--in`), the upload command always needs a scope.
2. **`artifact read` is text-only.** It decodes the body as UTF-8 and
   prints to stdout. For binary files use `attachment download` or `view`
   to write raw bytes.
3. **`view` truncates at `--max-bytes`.** If stderr shows a next offset,
   switch to `download` or raise `--max-bytes`.
4. **Publish body sources are mutually exclusive.** Pick exactly one of
   `--text`, `--file`, or stdin.
5. **Artifacts without `--in` are still accessible by id** but will not
   appear in scope-filtered listings.
6. **Long messages are truncated, not inline.** A 500-char prefix plus
   `[... full text attached as .txt ...]` means the real content is in
   the `message-*.txt` artifact — read it before acting (see above).
7. **`message read` without `--json` hides attachment ids.** The
   human-readable renderer prints only the body text. To see the
   structured `attachments` array, use `loom --json message read …` or
   parse the `[附件]` block from the body.
