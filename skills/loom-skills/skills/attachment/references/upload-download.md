# Upload & Download

Upload files into Loom scopes and download them back. Understand target
scoping, media types, and the upload-then-send pattern.

## Upload

```bash
loom attachment upload --target "#general" --path ./report.pdf
```

### Parameters

| Parameter | Required | Default | Description |
| --- | --- | --- | --- |
| `--target` | Yes | — | Channel (`#chan_…`), thread (`#chan_…:msg_…`), or DM (`dm:@actor_…`) |
| `--path` | Yes | — | Local file path to upload |
| `--mime-type` | No | `application/octet-stream` | Override MIME type |

### Output

```
Attachment ID: art_abc123…
```

Use `-J` (or `--json`) for structured JSON output when scripting:

```bash
loom attachment upload -J --target "#general" --path ./report.pdf
```

### Media type best practices

| File type | Recommended `--mime-type` |
| --- | --- |
| Markdown | `text/markdown` (default for `artifact publish`) |
| JSON | `application/json` |
| PDF | `application/pdf` |
| PNG image | `image/png` |
| Unknown binary | `application/octet-stream` (default) |

Setting the correct media type helps consumers decide whether to use
`artifact read` (text) or `attachment download` (binary).

## Target Scoping

The `--target` parameter controls where the artifact is anchored:

| Target form | Scope | Effect |
| --- | --- | --- |
| `#chan_…` | Channel | Artifact associated with the channel |
| `#chan_…:msg_…` | Thread | Artifact associated with a specific thread |
| `dm:@actor_…` | DM | Artifact associated with a direct message |

### Thread vs Channel target

- **Thread target** (`#chan_…:msg_…`): The artifact is scoped to the thread.
  It appears in thread-filtered listings and is the recommended target for
  workflow artifacts.
- **Channel target** (`#chan_…`): The artifact is scoped to the channel
  level. Use this for channel-wide resources not tied to a specific thread.

Always prefer the thread target (`$LOOM_REPLY_TARGET`) for workflow artifacts
to keep them associated with the active discussion.

## Upload-Then-Send Pattern

The standard workflow for attaching a file to a message:

```bash
# Step 1: Upload the file
ART_ID=$(loom attachment upload -J --target "#general" --path ./report.pdf \
  | python -c "import sys,json; print(json.load(sys.stdin)['artifact']['id'])")

# Step 2: Reference it in a message
loom message send --target "#general" --text "Report attached" \
  --attachment-id "$ART_ID"
```

### PowerShell variant (Windows)

```powershell
$art = loom attachment upload -J --target "#general" --path ./report.pdf | ConvertFrom-Json
loom message send --target "#general" --text "Report attached" --attachment-id $art.artifact.id
```

## Download

### Full download (auto-chunks until complete)

```bash
loom attachment download --id art_abc123 --output ./downloaded.pdf
```

The CLI always fetches the entire file regardless of size. `--chunk-bytes`
controls the internal loop size (default 1 MiB).

### Single-read view (one request, may truncate)

```bash
loom attachment view --id art_abc123 --output ./out.bin
```

| Parameter | Default | Description |
| --- | --- | --- |
| `--offset` | 0 | Start byte |
| `--max-bytes` | 10 MiB | Request cap. If truncated, next offset is printed to stderr. |

Use `view` for a byte-range peek; use `download` for the full payload.

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
