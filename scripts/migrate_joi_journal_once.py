#!/usr/bin/env python3
"""One-time Joi journal migration for the current schema.

This rewrites old `thread_create` + `thread_root_set` pairs into current
`thread_create` records that carry `rootEventId` directly. It can also append
`channel_grant` rows for actor-id aliases, useful when a human account moved
from a generated local actor id to a stable login actor id.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tempfile
import time
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("journal", type=Path, help="Path to data/journal.jsonl")
    parser.add_argument(
        "--actor-alias",
        action="append",
        default=[],
        metavar="OLD:NEW",
        help="Grant NEW to every live channel where OLD is currently a member",
    )
    parser.add_argument(
        "--write",
        action="store_true",
        help="Rewrite the journal in place. Without this, only print a dry-run summary.",
    )
    parser.add_argument(
        "--backup-suffix",
        default=None,
        help="Backup suffix. Defaults to migrate-once-<unix timestamp>.",
    )
    return parser.parse_args()


def compact(value: dict[str, Any]) -> str:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def get_op(record: dict[str, Any]) -> str | None:
    return record.get("op") or record.get("kind")


def get_data(record: dict[str, Any]) -> dict[str, Any]:
    data = record.get("data", record)
    return data if isinstance(data, dict) else {}


def get_id(data: dict[str, Any], snake: str, camel: str) -> str | None:
    value = data.get(snake) or data.get(camel)
    return value if isinstance(value, str) and value else None


def parse_aliases(raw_aliases: list[str]) -> list[tuple[str, str]]:
    aliases = []
    for raw in raw_aliases:
        old, sep, new = raw.partition(":")
        if not sep or not old or not new:
            raise SystemExit(f"invalid --actor-alias `{raw}`; expected OLD:NEW")
        aliases.append((old, new))
    return aliases


def load_records(path: Path) -> tuple[list[tuple[str, dict[str, Any] | None]], Counter[str]]:
    records: list[tuple[str, dict[str, Any] | None]] = []
    counts: Counter[str] = Counter()
    with path.open("r", encoding="utf-8") as handle:
        for line in handle:
            raw = line.rstrip("\n")
            try:
                record = json.loads(raw)
            except json.JSONDecodeError:
                records.append((raw, None))
                counts["malformed_kept"] += 1
                continue
            records.append((raw, record))
            counts[f"op:{get_op(record)}"] += 1
    return records, counts


def collect_state(
    records: list[tuple[str, dict[str, Any] | None]],
) -> tuple[dict[str, str], dict[str, dict[str, Any]], set[str], dict[str, list[str]]]:
    thread_roots: dict[str, str] = {}
    actors: dict[str, dict[str, Any]] = {}
    live_channels: set[str] = set()
    channel_members: dict[str, list[str]] = defaultdict(list)

    for _, record in records:
        if record is None:
            continue
        op = get_op(record)
        data = get_data(record)
        if op == "thread_root_set":
            thread_id = get_id(data, "thread_id", "threadId")
            root_event_id = get_id(data, "root_event_id", "rootEventId")
            if thread_id and root_event_id:
                thread_roots[thread_id] = root_event_id
        elif op == "actor_upsert":
            actor_id = data.get("id")
            if isinstance(actor_id, str) and actor_id:
                actors[actor_id] = data
        elif op == "channel_create":
            channel_id = data.get("id")
            if isinstance(channel_id, str) and channel_id:
                live_channels.add(channel_id)
                members = data.get("members", [])
                channel_members[channel_id] = [
                    m for m in members if isinstance(m, str) and m
                ]
        elif op == "channel_grant":
            channel_id = get_id(data, "channel_id", "channelId")
            actor_id = get_id(data, "actor_id", "actorId")
            if channel_id and actor_id and actor_id not in channel_members[channel_id]:
                channel_members[channel_id].append(actor_id)
        elif op == "channel_revoke":
            channel_id = get_id(data, "channel_id", "channelId")
            actor_id = get_id(data, "actor_id", "actorId")
            if channel_id and actor_id:
                channel_members[channel_id] = [
                    m for m in channel_members[channel_id] if m != actor_id
                ]
        elif op == "channel_delete":
            channel_id = get_id(data, "channel_id", "channelId")
            if channel_id:
                live_channels.discard(channel_id)

    return thread_roots, actors, live_channels, channel_members


def migrate_lines(
    records: list[tuple[str, dict[str, Any] | None]],
    thread_roots: dict[str, str],
) -> tuple[list[str], Counter[str]]:
    out: list[str] = []
    counts: Counter[str] = Counter()
    for raw, record in records:
        if record is None:
            out.append(raw)
            continue

        op = get_op(record)
        data = get_data(record)
        if op == "thread_root_set":
            counts["thread_root_set_removed"] += 1
            continue

        if op == "thread_create":
            thread_id = data.get("id")
            root = data.get("rootEventId") or data.get("root_event_id")
            if isinstance(thread_id, str) and not root:
                root = thread_roots.get(thread_id, "")
                data = dict(data)
                record = dict(record)
                record["data"] = data
                counts["thread_create_root_added"] += 1
            elif "root_event_id" in data:
                data = dict(data)
                record = dict(record)
                record["data"] = data
                counts["thread_create_root_renamed"] += 1

            if "root_event_id" in data:
                data.pop("root_event_id", None)
            data["rootEventId"] = root or ""
            out.append(compact(record))
            continue

        out.append(raw)
    return out, counts


def append_actor_alias_grants(
    out: list[str],
    aliases: list[tuple[str, str]],
    actors: dict[str, dict[str, Any]],
    live_channels: set[str],
    channel_members: dict[str, list[str]],
) -> Counter[str]:
    counts: Counter[str] = Counter()
    for old_actor, new_actor in aliases:
        if new_actor not in actors and old_actor in actors:
            actor = dict(actors[old_actor])
            actor["id"] = new_actor
            out.append(compact({"op": "actor_upsert", "data": actor}))
            actors[new_actor] = actor
            counts["actor_upsert_alias_added"] += 1

        for channel_id in sorted(live_channels):
            members = channel_members.get(channel_id, [])
            if old_actor not in members or new_actor in members:
                continue
            out.append(
                compact(
                    {
                        "op": "channel_grant",
                        "data": {"channel_id": channel_id, "actor_id": new_actor},
                    }
                )
            )
            members.append(new_actor)
            counts["channel_grant_alias_added"] += 1
    return counts


def write_in_place(path: Path, lines: list[str], backup_suffix: str) -> Path:
    backup = path.with_name(f"{path.name}.bak.{backup_suffix}")
    shutil.copy2(path, backup)
    fd, tmp_name = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            for line in lines:
                handle.write(line)
                handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(tmp_name, path)
    except Exception:
        try:
            os.unlink(tmp_name)
        except FileNotFoundError:
            pass
        raise
    return backup


def main() -> int:
    args = parse_args()
    aliases = parse_aliases(args.actor_alias)
    records, input_counts = load_records(args.journal)
    thread_roots, actors, live_channels, channel_members = collect_state(records)
    lines, migration_counts = migrate_lines(records, thread_roots)
    alias_counts = append_actor_alias_grants(
        lines, aliases, actors, live_channels, channel_members
    )

    print(f"journal={args.journal}")
    print(f"input_lines={len(records)} output_lines={len(lines)}")
    for name, value in (input_counts + migration_counts + alias_counts).most_common():
        print(f"{name}={value}")

    if not args.write:
        print("dry_run=true")
        return 0

    suffix = args.backup_suffix or f"migrate-once-{int(time.time())}"
    backup = write_in_place(args.journal, lines, suffix)
    print(f"dry_run=false")
    print(f"backup={backup}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
