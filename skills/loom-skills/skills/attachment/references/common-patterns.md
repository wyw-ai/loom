# Common Patterns

Real-world attachment and artifact patterns derived from Loom iteration
experience. Each pattern includes the scenario, commands, and platform notes.

## Pattern 1: Research Report Publishing

**Scenario**: An agent produces a research report (markdown, 10–30 KB) that
needs to be shared in a thread and referenced by other actors.

### Workflow

```bash
# Step 1: Publish the report as an artifact in the thread scope
loom artifact publish --in "$LOOM_SCOPE_ID" \
  --name "strat-competitor-analysis.md" \
  --file ./report.md

# Output: art_abc123    text/markdown    28672 bytes    artifact://...

# Step 2: Notify the thread with the artifact reference
loom message send --target "$LOOM_REPLY_TARGET" \
  --text "STRAT competitor analysis published. Key finding: ..." \
  --attachment-id art_abc123
```

### Why not just paste the text?

- Reports > 4000 chars get truncated in message bodies.
- Artifacts are addressable by id — other actors can read them on demand.
- The artifact persists across session resets.

### Variant: Publish from stdin (for generated content)

```bash
# Generate report and publish in one pipeline
generate-report.sh | loom artifact publish --in "$LOOM_SCOPE_ID" \
  --name "generated-report.md"
```

## Pattern 2: Windows Multiline Messages

**Scenario**: On Windows/PowerShell, you need to send a multiline message
body. Loom stores message text literally — escaped `\n` sequences appear as
literal backslash-n, not newlines.

### PowerShell here-string pattern

```powershell
# Use here-strings for multiline text
$body = @'
Line 1 of the message.
Line 2 of the message.
Line 3 with "quotes" and special chars.
'@

loom message send --target $LOOM_REPLY_TARGET --text $body
```

### PowerShell pipe to artifact publish

```powershell
# Publish multiline content from a here-string
$content = @'
# Report Title

Section 1 content here.

Section 2 content here.
'@

$content | loom artifact publish --in $env:LOOM_SCOPE_ID --name "report.md"
```

### Key rules

| Platform | Multiline method | Avoid |
| --- | --- | --- |
| **PowerShell (Windows)** | Here-string `@'...'@` or `@"..."@` | `\n` in `--text` (literal backslash-n) |
| **Bash (Linux/Mac)** | Heredoc `<<'EOF'...EOF` or `$'...\n...'` | `\n` in single-quoted strings |

### Cross-platform note

Loom stores message text literally. Always pass real newline characters, not
escaped `\n` sequences. The `--allow-escaped-newlines` flag exists for
backward compatibility but should not be used for new content.

## Pattern 3: Paginated Reading Loop

**Scenario**: You need to read a large artifact (27 KB+ research report) that
exceeds the default 64 KiB read limit, or you want to process it in chunks.

### Manual pagination

```bash
# Read first 4 KB
loom artifact read art_abc123 --offset 0 --max-bytes 4096 -J
# → { "content": "...", "nextOffset": 4096 }

# Read next 4 KB
loom artifact read art_abc123 --offset 4096 --max-bytes 4096 -J
# → { "content": "...", "nextOffset": 8192 }

# Continue until nextOffset is null
```

### Bash loop (Linux/Mac)

```bash
OFFSET=0
while true; do
  RESULT=$(loom artifact read art_abc123 --offset $OFFSET --max-bytes 8192 -J)
  echo "$RESULT" | python -c "import sys,json; d=json.load(sys.stdin); print(d.get('content',''),end='')"
  NEXT=$(echo "$RESULT" | python -c "import sys,json; print(json.load(sys.stdin).get('nextOffset') or '')")
  if [ -z "$NEXT" ]; then break; fi
  OFFSET=$NEXT
done
```

### PowerShell loop (Windows)

```powershell
$offset = 0
while ($true) {
    $result = loom artifact read art_abc123 --offset $offset --max-bytes 8192 -J | ConvertFrom-Json
    Write-Host $result.content -NoNewline
    if (-not $result.nextOffset) { break }
    $offset = $result.nextOffset
}
```

### When to paginate vs download

| Situation | Recommended approach |
| --- | --- |
| Text artifact, need to process in chunks | Paginated `artifact read` |
| Text artifact, need full content at once | Single `artifact read` (if < 64 KiB) or `attachment download` |
| Binary artifact (PDF, image) | `attachment download` (always) |

## Pattern 4: Report Archival (Loom → Obsidian)

**Scenario**: After completing a workflow, archive Loom-published artifacts
to the Obsidian knowledge vault for long-term storage.

### Workflow

```bash
# Step 1: Read the artifact content
loom artifact read art_abc123 > ./report.md

# Step 2: Copy to Obsidian vault
cp ./report.md "/path/to/obsidian/vault/project-folder/report.md"

# Step 3: Verify sync
diff <(loom artifact read art_abc123) "/path/to/obsidian/vault/project-folder/report.md"
```

### PowerShell variant

```powershell
# Read and save
loom artifact read art_abc123 | Out-File -FilePath "./report.md" -Encoding utf8

# Copy to vault
Copy-Item "./report.md" "C:\Users\hansi\OneDrive\note\ai_note\ai-note\project-folder\report.md"
```

### Archival checklist

- [ ] Artifact content downloaded/read completely
- [ ] File saved to Obsidian vault with descriptive name
- [ ] Frontmatter (title, date, tags) added if missing
- [ ] Wikilinks to related notes added
- [ ] Original artifact id noted for traceability

## Pattern 5: Long-Message Truncation Recovery

**Scenario**: A human sends a long message (≥ 4000 chars) from the GUI. The
body is truncated and the full text is stored as a `message-*.txt` artifact.

### Recognition signals

| Signal | Meaning |
| --- | --- |
| `[... full text attached as .txt ...]` | Original body converted to `message-*.txt` artifact |
| `[附件]` block with `[artifact: art_…]` | One or more artifacts attached |
| `(message body truncated: showing N of M chars…)` | Runtime truncation |

### Recovery workflow

```bash
# Step 1: Extract artifact ids from the [附件] block
# Format: - <name> (<mediaType>, <size>) [artifact: <art_id>]

# Step 2: Read each text artifact
loom artifact read art_f5fcf973035f

# For large artifacts, paginate:
loom artifact read art_f5fcf973035f --offset 0 --max-bytes 65536

# Step 3: For binary artifacts, download
loom attachment download --id art_03f415a21d1c --output ./attachment.bin
```

### Critical rule

**Whenever you see `[... full text attached as .txt ...]` or a `[附件]` block,
the message you received is incomplete.** You MUST fetch the full content
before acting. Failing to do so means working from a 500-char prefix of a
message that may be thousands of characters long.
