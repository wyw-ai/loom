#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any


POLL_INTERVAL = int(os.environ.get("MR_WATCHER_INTERVAL", "60"))
JOI_SERVER = os.environ.get("JOI_SERVER", "ws://127.0.0.1:7878/rpc")
ACTOR_ID = os.environ.get("MR_WATCHER_ACTOR_ID", "mr-watcher")
STATE_DIR = Path(os.environ.get("MR_WATCHER_STATE_DIR", str(Path.home() / ".local/state/joi-agent/mr-watcher")))
STATE_FILE = STATE_DIR / "state.json"
TERMINAL_FILE = STATE_DIR / "terminal.json"
CURRENT_RUN_GROUP_MAX_ID_GAP = int(os.environ.get("MR_WATCHER_CURRENT_RUN_GROUP_MAX_ID_GAP", "1000"))
COMPENSATION_SECONDS = int(os.environ.get("MR_WATCHER_COMPENSATION_SECONDS", "1800"))
QUIET_SECONDS = int(os.environ.get("MR_WATCHER_QUIET_SECONDS", "300"))
JOI_RPC_HELPER = Path(
    os.environ.get(
        "JOI_RPC_HELPER",
        str(Path(__file__).resolve().parents[2] / "dev-helper-bridge" / "joi_rpc.py"),
    )
)
EVENT_SCAN_LIMIT = int(os.environ.get("MR_WATCHER_EVENT_SCAN_LIMIT", "120"))

DELIVERY_SELF_USERNAMES = {
    item.strip()
    for item in os.environ.get("MR_WATCHER_DELIVERY_SELF_USERNAMES", "chutianshu.cts").split(",")
    if item.strip()
}

MR_URL_RE = re.compile(
    r"https://code\.alibaba-inc\.com/(?P<repo>[^/\s]+/[^/\s]+)/(?:merge_requests/(?P<iid>\d+)|codereview/(?P<id>\d+))"
)
BRANCH_RE = re.compile(r"(?:branch|分支)[：:]\s*`?(?P<branch>[^`\s]+)")
COMMENTS_RE = re.compile(r"(?:comments|评论数)[：:]\s*(?P<count>\d+)")
MR_OPENED_BLOCK_RE = re.compile(r"\[mr-opened v1\](?P<body>.*?)\[/mr-opened v1\]", re.S)
EXAMINER_RESULT_RE = re.compile(r"\[examiner-result\](?P<body>.*?)\[/examiner-result\]", re.S)



@dataclass
class MrWatch:
    thread_id: str
    repo: str
    mr_url: str
    mr_id: int | None = None
    mr_iid: int | None = None
    branch: str | None = None
    target_branch: str | None = None
    work_item_id: str | None = None
    comment_ids: list[int] = field(default_factory=list)
    seen_note_keys: list[str] = field(default_factory=list)
    last_test_ok: bool | None = None
    last_approval_ok: bool | None = None
    last_discussion_ok: bool | None = None
    last_ready_to_merge: bool | None = None
    last_state: str | None = None
    last_ci_summary: str | None = None
    last_ci_signature: str | None = None
    last_ci_reported_at: str | None = None
    last_conflict: bool | None = None
    merged_emitted: bool = False
    closed_emitted: bool = False
    pending_report: dict = field(default_factory=dict)
    pending_report_signature: str | None = None
    pending_report_since: str | None = None
    pending_report_last_changed_at: str | None = None
    pending_report_last_changed_epoch: float | None = None
    # Persistent ledger keyed by note_id. Each entry:
    # {author, username, role, parent_note_id, kind: user_request|self_post,
    #  body_sha1, body_preview, classified_at, forwarded_at, replied_with}
    comments: dict = field(default_factory=dict)


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Poll MR state and translate it into Joi thread events.")
    p.add_argument("--once", action="store_true", help="Run one scan and exit.")
    p.add_argument("--thread-id", action="append", help="Limit polling to one or more thread ids.")
    p.add_argument("--allow-global", action="store_true", help="Allow legacy global polling without --thread-id.")
    return p.parse_args()


def run(cmd: list[str], *, cwd: str | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, text=True, capture_output=True, cwd=cwd, check=False)


def run_json(cmd: list[str], *, cwd: str | None = None) -> Any:
    proc = run(cmd, cwd=cwd)
    if proc.returncode != 0:
        raise RuntimeError((proc.stderr or proc.stdout).strip() or f"command failed: {' '.join(cmd)}")
    return json.loads(proc.stdout)


def now_iso() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def iso_to_epoch(value: str | None) -> float | None:
    if not value:
        return None
    try:
        return time.mktime(time.strptime(value, "%Y-%m-%dT%H:%M:%SZ"))
    except Exception:
        return None


def compensation_due(last_reported_at: str | None) -> bool:
    last_epoch = iso_to_epoch(last_reported_at)
    if last_epoch is None:
        return True
    return (time.time() - last_epoch) >= COMPENSATION_SECONDS


def watch_key(watch: MrWatch) -> str:
    if watch.mr_url:
        return f"{watch.thread_id}::{watch.repo}::{watch.mr_url}"
    if watch.mr_id is not None:
        return f"{watch.thread_id}::{watch.repo}::mrid:{watch.mr_id}"
    if watch.mr_iid is not None:
        return f"{watch.thread_id}::{watch.repo}::mriid:{watch.mr_iid}"
    return f"{watch.thread_id}::{watch.repo}::{watch.branch or '-'}::{watch.target_branch or '-'}"


def watch_owner_key(watch: MrWatch) -> str:
    if watch.mr_id is not None:
        return f"{watch.repo}::mrid:{watch.mr_id}"
    if watch.mr_url:
        return f"{watch.repo}::{watch.mr_url}"
    if watch.mr_iid is not None:
        return f"{watch.repo}::mriid:{watch.mr_iid}"
    return f"{watch.repo}::{watch.branch or '-'}::{watch.target_branch or '-'}"


def terminal_key(watch: MrWatch) -> str:
    if watch.mr_id is not None:
        return f"{watch.thread_id}::{watch.repo}::mrid:{watch.mr_id}"
    if watch.mr_url:
        return f"{watch.thread_id}::{watch.repo}::{watch.mr_url}"
    return watch_key(watch)


def load_terminal() -> dict[str, dict[str, Any]]:
    if not TERMINAL_FILE.exists():
        return {}
    try:
        payload = json.loads(TERMINAL_FILE.read_text())
    except Exception:
        return {}
    return payload if isinstance(payload, dict) else {}


def save_terminal(payload: dict[str, dict[str, Any]]) -> None:
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    TERMINAL_FILE.write_text(json.dumps(payload, ensure_ascii=False, indent=2))


def mark_terminal(watch: MrWatch, kind: str) -> None:
    payload = load_terminal()
    payload[terminal_key(watch)] = {
        "thread_id": watch.thread_id,
        "repo": watch.repo,
        "mr_url": watch.mr_url,
        "mr_id": watch.mr_id,
        "branch": watch.branch,
        "terminal_kind": kind,
        "terminal_at": now_iso(),
    }
    save_terminal(payload)


def load_state() -> dict[str, MrWatch]:
    if not STATE_FILE.exists():
        return {}
    raw = json.loads(STATE_FILE.read_text())
    state: dict[str, MrWatch] = {}
    for _, payload in raw.items():
        watch = MrWatch(**payload)
        state[watch_key(watch)] = watch
    return state


def save_state(state: dict[str, MrWatch]) -> None:
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    payload = {watch_key(watch): asdict(watch) for watch in state.values()}
    STATE_FILE.write_text(json.dumps(payload, ensure_ascii=False, indent=2))


def list_threads() -> list[dict[str, Any]]:
    data = run_json(["joi", "--json", "thread", "list"])
    return data.get("threads") or []


def list_events(thread_id: str, limit: int = EVENT_SCAN_LIMIT) -> list[dict[str, Any]]:
    data = run_json(["joi", "--json", "event", "list", "--in", thread_id, "--limit", str(limit)])
    return data.get("events") or []


def warn(payload: dict[str, Any]) -> None:
    sys.stderr.write(json.dumps(payload, ensure_ascii=False) + "\n")
    sys.stderr.flush()


def is_missing_or_terminal_mr_error(exc: Exception) -> bool:
    message = str(exc).lower()
    patterns = (
        "no open merge request found",
        "unable to resolve mr id",
        "merge request not found",
        "mr not found",
        "404",
    )
    return any(pattern in message for pattern in patterns)


def should_drop_terminal_state(watch: MrWatch, exc: Exception) -> bool:
    if not is_missing_or_terminal_mr_error(exc):
        return False
    return bool(watch.mr_url or watch.mr_id is not None or watch.branch)


def scope_state_info(thread_id: str) -> dict[str, Any]:
    scope_state = Path.home() / ".local/state/joi-agent/scope-projects" / f"thread-{thread_id}.json"
    if scope_state.exists():
        try:
            return json.loads(scope_state.read_text())
        except json.JSONDecodeError:
            return {}
    return {}


def hydrate_watch(thread_id: str, watch: MrWatch) -> MrWatch:
    info = scope_state_info(thread_id)
    watch.work_item_id = watch.work_item_id or info.get("work_item_id") or None
    return watch


def first_work_item_id(fields: dict[str, str]) -> str | None:
    for key in ("work_item_id", "workitem_id", "feedback_id", "work_item_ids", "work-items"):
        value = (fields.get(key) or "").strip()
        if not value:
            continue
        hit = re.search(r"\d+", value)
        return hit.group(0) if hit else value
    return None


def parse_mr_block(thread_id: str, block_body: str) -> MrWatch | None:
    fields: dict[str, str] = {}
    for raw_line in block_body.splitlines():
        line = raw_line.strip()
        if not line or ":" not in line:
            continue
        key, value = line.split(":", 1)
        fields[key.strip()] = value.strip()
    repo = fields.get("repo")
    mr_url = fields.get("mr_url")
    source_branch = fields.get("source_branch")
    target_branch = fields.get("target_branch")
    if not (repo and mr_url and source_branch and target_branch):
        return None
    url_hit = MR_URL_RE.search(mr_url)
    url_id = int(url_hit.group("id")) if url_hit and url_hit.group("id") else None
    url_iid = int(url_hit.group("iid")) if url_hit and url_hit.group("iid") else None
    return hydrate_watch(
        thread_id,
        MrWatch(
            thread_id=thread_id,
            repo=repo,
            mr_url=mr_url,
            mr_id=int(fields["mr_id"]) if fields.get("mr_id") else url_id,
            mr_iid=int(fields["mr_iid"]) if fields.get("mr_iid") else url_iid,
            branch=source_branch,
            target_branch=target_branch,
            work_item_id=first_work_item_id(fields),
        ),
    )


def parse_mr_watches(thread_id: str, events: list[dict[str, Any]]) -> list[MrWatch]:
    watches: dict[str, MrWatch] = {}
    for event in events:
        payload = event.get("payload") or {}
        text = payload.get("text") or ""
        for block in MR_OPENED_BLOCK_RE.finditer(text):
            watch = parse_mr_block(thread_id, block.group("body"))
            if watch:
                watches[watch_key(watch)] = watch

    for event in reversed(events):
        payload = event.get("payload") or {}
        text = payload.get("text") or ""
        branch_match = BRANCH_RE.search(text)
        branch = branch_match.group("branch") if branch_match else None
        target_branch = "master" if "→ master" in text else None
        for hit in MR_URL_RE.finditer(text):
            repo = hit.group("repo")
            mr_url = hit.group(0)
            mr_id = int(hit.group("id")) if hit.group("id") else None
            mr_iid = int(hit.group("iid")) if hit.group("iid") else None
            watch = hydrate_watch(
                thread_id,
                MrWatch(
                    thread_id=thread_id,
                    repo=repo,
                    mr_url=mr_url,
                    mr_id=mr_id,
                    mr_iid=mr_iid,
                    branch=branch,
                    target_branch=target_branch,
                ),
            )
            watches.setdefault(watch_key(watch), watch)
    return list(watches.values())


def resolve_mr_id(watch: MrWatch) -> MrWatch:
    if watch.mr_id is not None:
        return watch
    candidates = run_json(["a1", "-f", "json", "repo", "mr", "list", "--repo", watch.repo])
    if not isinstance(candidates, list):
        raise RuntimeError(f"unexpected MR list output for repo {watch.repo}")
    for item in candidates:
        if watch.mr_iid is not None and item.get("iid") == watch.mr_iid:
            watch.mr_id = item.get("id")
            watch.branch = watch.branch or item.get("sourceBranch")
            watch.target_branch = watch.target_branch or item.get("targetBranch")
            return watch
        if item.get("detailUrl") and item.get("detailUrl") == watch.mr_url:
            watch.mr_id = item.get("id")
            watch.mr_iid = watch.mr_iid or item.get("iid")
            watch.branch = watch.branch or item.get("sourceBranch")
            watch.target_branch = watch.target_branch or item.get("targetBranch")
            return watch
        if item.get("webUrl") and item.get("webUrl") == watch.mr_url:
            watch.mr_id = item.get("id")
            watch.mr_iid = watch.mr_iid or item.get("iid")
            watch.branch = watch.branch or item.get("sourceBranch")
            watch.target_branch = watch.target_branch or item.get("targetBranch")
            return watch
        if watch.branch and item.get("sourceBranch") == watch.branch:
            watch.mr_id = item.get("id")
            watch.mr_iid = watch.mr_iid or item.get("iid")
            watch.target_branch = watch.target_branch or item.get("targetBranch")
            return watch
    raise RuntimeError(f"unable to resolve mr id for {watch.mr_url}")


def read_mr_view(watch: MrWatch) -> dict[str, Any]:
    assert watch.mr_id is not None
    data = run_json(["a1", "-f", "json", "repo", "mr", "view", "--repo", watch.repo, str(watch.mr_id)])
    if isinstance(data, dict) and "mergeRequest" in data:
        mr = data["mergeRequest"]
        watch.mr_iid = watch.mr_iid or mr.get("iid")
        watch.branch = watch.branch or mr.get("sourceBranch")
        watch.target_branch = watch.target_branch or mr.get("targetBranch")
        if mr.get("detailUrl"):
            watch.mr_url = mr["detailUrl"]
        return mr
    raise RuntimeError(f"unexpected MR view output for {watch.repo}#{watch.mr_id}")


def read_mr_status(watch: MrWatch) -> dict[str, Any]:
    cmd = ["a1", "-f", "json", "repo", "mr", "status", "--repo", watch.repo]
    if watch.mr_id is not None:
        cmd.append(str(watch.mr_id))
    elif watch.branch:
        cmd += ["--source", watch.branch]
        if watch.target_branch:
            cmd += ["--target", watch.target_branch]
    else:
        raise RuntimeError(f"missing mr_id/source branch for {watch.repo} {watch.mr_url}")
    return run_json(cmd)


def list_comments(watch: MrWatch) -> list[dict[str, Any]]:
    assert watch.mr_id is not None
    data = run_json(["a1", "-f", "json", "repo", "mr", "comment", "list", "--repo", watch.repo, "--mr", str(watch.mr_id)])
    if isinstance(data, list):
        return data
    raise RuntimeError(f"unexpected MR comments output for {watch.repo}#{watch.mr_id}")


def append_event(
    event_type: str, thread_id: str, text: str, *, handoff: str | None, payload: dict[str, Any], mr_urls: list[str] | None = None
) -> None:
    cmd = [
        "python3",
        str(JOI_RPC_HELPER),
        "append",
        "--scope-kind",
        "thread",
        "--scope-id",
        thread_id,
        "--actor-id",
        ACTOR_ID,
        "--text",
        text,
    ]
    if handoff:
        cmd += ["--handoff", handoff]
    proc = run(cmd)
    if proc.returncode != 0:
        raise RuntimeError((proc.stderr or proc.stdout).strip() or f"append failed for {thread_id}")
    # Follow the helper with a structured JSON line for debugging/log shipping.
    sys.stdout.write(
        json.dumps(
            {
                "event_type": event_type,
                "thread_id": thread_id,
                "mr_urls": mr_urls or [],
                "payload": payload,
            },
            ensure_ascii=False,
        )
        + "\n"
    )
    sys.stdout.flush()


def bool_from_checks(status: dict[str, Any], check_type: str) -> tuple[bool | None, str | None]:
    checks = status.get("checks") or []
    for item in checks:
        if item.get("checkType") == check_type:
            return bool(item.get("checkResult")), item.get("checkName")
    return None, None


def note_text(note: dict[str, Any]) -> str:
    return (
        str(note.get("note") or note.get("text") or note.get("body") or note.get("content") or "")
        .strip()
        .replace("\n", " ")
    )



NOISE_AUTHOR_PATTERNS = [
    re.compile(r'^(gitlab[- ]?bot|aone[- ]?bot|ci[- ]?bot|system|robot|机器人)$', re.I),
]
# Patterns that mark a comment body as bot self-talk. Matched anywhere in the
# body, case-insensitive.
BOT_SELFTALK_PATTERNS = [
    re.compile(r'^\s*LGTM\s*-\s*actor_examiner\s*$', re.I | re.M),
    re.compile(r'\[mr-opened v1\]', re.I),
    re.compile(r'MR 扫描报告', re.I),
    re.compile(r'→ handoff →', re.I),
    re.compile(r'^\s*(done in [0-9a-f]{6,}|ack|ok|收到|好的|thanks?)\s*[!.。]?\s*$', re.I | re.M),
    re.compile(r'\b(still green|no action needed|status check[:.]?|all ci (pipelines? )?are? green|all ci runs? now green|all ci (pipelines? )?green)\b', re.I),
    re.compile(r'^\s*✅\s*(all ci|golang|status|ci|run\#)', re.I | re.M),
    re.compile(r'\bbenchmark report posted\b', re.I),
]
DELIVERY_SELF_REPLY_PATTERNS = [
    re.compile(r'已在后续\s*commit\s*中?修复', re.I),
    re.compile(r'commit\s+[0-9a-f]{7,}', re.I),
    re.compile(r'最新\s*commit', re.I),
    re.compile(r'CI\s*已通过|test\s*门禁已通过|门禁.*✅', re.I),
    re.compile(r'Golang\s*代码扫描.*(已通过|SUCCESS|修复)', re.I),
    re.compile(r'failure\s*已.*修复|失败已.*修复', re.I),
    re.compile(r'^\s*感谢.*(审阅|approve|LGTM)', re.I),
    re.compile(r'MR\s*描述已包含|代码改动已完成', re.I),
    re.compile(r'请问.*具体.*(页面|指|问题|位置)', re.I),
    re.compile(r'已回复|done in\s+[0-9a-f]{6,}', re.I),
]
# When a body matches any of these, it is ALWAYS treated as user→bot request.
USER_MENTION_PATTERNS = [
    re.compile(r'@\s*残风的Agent', re.I),
    re.compile(r'@\s*残风Agent', re.I),
    re.compile(r'@\s*chutianshu\.cts\.bot', re.I),
    re.compile(r'@\s*aone-?bot', re.I),
]

ROUTER_ESCALATION_PATTERNS = [
    re.compile(r'实现不合理|方案不合理|不属于bug|不属于 bug|不是bug|不是 bug', re.I),
    re.compile(r'可以废弃|应该废弃|废弃这个\s*mr|mr\s*.*废弃|关闭\s*MR|撤回\s*MR', re.I),
    re.compile(r'建一个需求|创建需求|关联到.*需求|当前对应的缺陷关联', re.I),
    re.compile(r'本质上应该|应该要加|方向不对|需求过时|缺陷不成立', re.I),
]


import hashlib


def _matches_any(text, pats) -> bool:
    t = text or ""
    return any(p.search(t) for p in pats)


def is_user_mention(text: str) -> bool:
    return _matches_any(text, USER_MENTION_PATTERNS)


def is_bot_selftalk(text: str) -> bool:
    return _matches_any(text, BOT_SELFTALK_PATTERNS)


def is_delivery_self_reply(note: dict[str, Any], watch: MrWatch) -> bool:
    username = (note.get("reviewer_username") or "").strip()
    if username not in DELIVERY_SELF_USERNAMES:
        return False
    text = note.get("note") or ""
    if is_user_mention(text):
        return False
    if not _matches_any(text, DELIVERY_SELF_REPLY_PATTERNS):
        return False
    if note.get("role") != "reply":
        return True
    parent = str(note.get("parent_note_id") or "")
    root = str(note.get("root_note_id") or "")
    comments = watch.comments or {}
    parent_was_forwarded = bool(comments.get(parent, {}).get("forwarded_at"))
    root_was_forwarded = bool(comments.get(root, {}).get("forwarded_at"))
    return parent_was_forwarded or root_was_forwarded


def needs_router_escalation(text: str) -> bool:
    return _matches_any(text, ROUTER_ESCALATION_PATTERNS)


def parse_examiner_result(text: str) -> dict[str, str] | None:
    match = EXAMINER_RESULT_RE.search(text or "")
    if not match:
        return None
    result: dict[str, str] = {}
    for raw in match.group("body").splitlines():
        line = raw.strip()
        if not line or "=" not in line:
            continue
        key, value = line.split("=", 1)
        result[key.strip()] = value.strip()
    if result.get("gate") != "mr_review":
        return None
    return result


def examiner_action_target(result: dict[str, str]) -> str:
    verdict = result.get("verdict", "")
    explicit = result.get("action_target", "")
    if explicit in {"delivery", "router", "none"}:
        return explicit
    if verdict == "needs_changes":
        return "delivery"
    if verdict in {"design_review_needed", "reject"}:
        return "router"
    return "none"


def classify_note_kind(author: str, text: str) -> str:
    """Return 'user_request' | 'self_post'.

    Bot and human share an Aone account, so author is unreliable. Classify by
    body content. Explicit @-mention of the bot is always user_request; bot
    self-talk patterns force self_post; everything else defaults to
    user_request (better to over-forward than miss a real ask).
    """
    a = (author or '').strip()
    t = (text or '').strip()
    if not t:
        return "self_post"
    if parse_examiner_result(t) is not None:
        return "user_request"
    if is_user_mention(t):
        return "user_request"
    for pat in NOISE_AUTHOR_PATTERNS:
        if pat.match(a):
            return "self_post"
    if is_bot_selftalk(t):
        return "self_post"
    return "user_request"


def body_sha1(text: str) -> str:
    return hashlib.sha1((text or "").encode("utf-8")).hexdigest()[:16]


def note_parent_id(comment: dict[str, Any]) -> int | None:
    raw = (
        comment.get("parentNoteId")
        or comment.get("parent_note_id")
        or comment.get("parentId")
        or comment.get("parent_id")
    )
    try:
        value = int(raw)
    except Exception:
        return None
    return value or None


def comment_root_map(comments: list[dict[str, Any]]) -> dict[int, int]:
    parent_by_id: dict[int, int | None] = {}
    for comment in comments:
        if not isinstance(comment, dict):
            continue
        try:
            cid = int(comment["id"])
        except Exception:
            continue
        parent_by_id[cid] = note_parent_id(comment)

    roots: dict[int, int] = {}

    def root_for(note_id: int) -> int:
        seen: set[int] = set()
        cur = note_id
        while True:
            parent = parent_by_id.get(cur)
            if not parent or parent in seen:
                return cur
            seen.add(cur)
            cur = parent

    for cid in parent_by_id:
        roots[cid] = root_for(cid)
    return roots


def flatten_notes(comment: dict[str, Any], parent_id: int | None = None, root_id: int | None = None) -> list[dict[str, Any]]:
    flattened: list[dict[str, Any]] = []
    try:
        current_id = int(comment["id"])
    except Exception:
        return flattened
    explicit_parent = note_parent_id(comment)
    if parent_id is None:
        parent_id = explicit_parent
    if root_id is None:
        root_id = current_id if parent_id is None else parent_id

    author = (comment.get("author") or {}).get("name") or "unknown"
    author_username = (comment.get("author") or {}).get("username") or ""
    text = note_text(comment)
    flattened.append(
        {
            "note_key": f"note:{current_id}",
            "note_id": current_id,
            "parent_note_id": parent_id,
            "root_note_id": root_id,
            "role": "reply" if parent_id is not None else "comment",
            "reviewer": author,
            "reviewer_username": author_username,
            "note": text,
            "closed": bool(comment.get("closed") or comment.get("resolved")),
        }
    )

    children = comment.get("replies") or comment.get("children") or []
    for child in children:
        if isinstance(child, dict):
            flattened.extend(flatten_notes(child, parent_id=current_id, root_id=root_id or current_id))
    return flattened


def collect_new_notes(watch: MrWatch, comments: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Walk comments, persist a per-note ledger, return user_request notes
    that have not been forwarded yet. Once classified, sticky in
    watch.comments[<note_id>]. Pre-existing seen ids (before ledger upgrade)
    are bootstrapped as self_post / forwarded so we don't flood delivery."""
    if watch.comments is None:
        watch.comments = {}
    bootstrap = not watch.comments and (watch.seen_note_keys or watch.comment_ids)
    bootstrap_ids: set[str] = set()
    root_by_id = comment_root_map(comments)
    if bootstrap:
        for k in watch.seen_note_keys:
            if isinstance(k, str) and k.startswith("note:"):
                bootstrap_ids.add(k.split(":", 1)[1])
        for cid in watch.comment_ids:
            bootstrap_ids.add(str(cid))

    new_notes: list[dict[str, Any]] = []
    for comment in comments:
        for note in flatten_notes(comment):
            note["root_note_id"] = root_by_id.get(note["note_id"], note.get("root_note_id") or note["note_id"])
            note_id = str(note["note_id"])
            if note["note_key"] not in watch.seen_note_keys:
                watch.seen_note_keys.append(note["note_key"])
            if note["role"] == "comment" and note["note_id"] not in watch.comment_ids:
                watch.comment_ids.append(note["note_id"])

            entry = watch.comments.get(note_id)
            text = note.get("note") or ""
            if entry is None:
                if note_id in bootstrap_ids:
                    if is_user_mention(text):
                        kind = "user_request"
                        forwarded_at = None
                    else:
                        kind = "self_post"
                        forwarded_at = "bootstrap"
                else:
                    if is_delivery_self_reply(note, watch):
                        kind = "self_post"
                        forwarded_at = now_iso()
                    else:
                        kind = classify_note_kind(note.get("reviewer") or "", text)
                        forwarded_at = None
                entry = {
                    "author": note.get("reviewer") or "",
                    "username": note.get("reviewer_username") or "",
                    "role": note["role"],
                    "parent_note_id": note.get("parent_note_id"),
                    "root_note_id": note.get("root_note_id"),
                    "kind": kind,
                    "body_sha1": body_sha1(text),
                    "body_preview": text[:200],
                    "classified_at": now_iso(),
                    "forwarded_at": forwarded_at,
                    "replied_with": None,
                }
                watch.comments[note_id] = entry
            else:
                new_sha = body_sha1(text)
                if entry.get("body_sha1") != new_sha:
                    entry["body_sha1"] = new_sha
                    entry["body_preview"] = text[:200]
                    if entry.get("kind") == "self_post" and is_user_mention(text):
                        entry["kind"] = "user_request"
                        entry["forwarded_at"] = None
                        entry["classified_at"] = now_iso()

            if entry.get("kind") == "user_request" and not entry.get("forwarded_at"):
                out = dict(note)
                out["kind"] = note["role"]
                out["ledger_kind"] = entry["kind"]
                new_notes.append(out)

    return new_notes


def mark_notes_forwarded(watch: MrWatch, notes: list[dict[str, Any]]) -> None:
    ts = now_iso()
    for n in notes or []:
        nid = str(n.get("note_id"))
        e = (watch.comments or {}).get(nid)
        if e is not None and not e.get("forwarded_at"):
            e["forwarded_at"] = ts


def has_debounceable_delta(entry: dict[str, Any]) -> bool:
    if entry.get("merged") or entry.get("closed"):
        return False
    return bool(entry.get("new_notes_count") or entry.get("ci_issues") or entry.get("conflict"))


def debounce_signature(entry: dict[str, Any]) -> str:
    payload = {
        "notes": sorted(entry.get("new_note_ids") or []),
        "ci_signature": entry.get("ci_signature") or "",
        "conflict": entry.get("conflict") or None,
    }
    return json.dumps(payload, ensure_ascii=False, sort_keys=True)


def clear_pending_report(watch: MrWatch) -> None:
    watch.pending_report = {}
    watch.pending_report_signature = None
    watch.pending_report_since = None
    watch.pending_report_last_changed_at = None
    watch.pending_report_last_changed_epoch = None


def debounced_entry_if_due(watch: MrWatch, entry: dict[str, Any]) -> dict[str, Any] | None:
    now = now_iso()
    now_epoch = time.time()
    signature = debounce_signature(entry)

    if not watch.pending_report:
        watch.pending_report = entry
        watch.pending_report_signature = signature
        watch.pending_report_since = now
        watch.pending_report_last_changed_at = now
        watch.pending_report_last_changed_epoch = now_epoch
        warn({
            "thread_id": watch.thread_id,
            "stage": "debounce_queue",
            "mr_id": watch.mr_id,
            "quiet_seconds": QUIET_SECONDS,
            "signature": signature,
        })
        return None

    if watch.pending_report_signature != signature:
        watch.pending_report_signature = signature
        watch.pending_report_last_changed_at = now
        watch.pending_report_last_changed_epoch = now_epoch
        warn({
            "thread_id": watch.thread_id,
            "stage": "debounce_extend",
            "mr_id": watch.mr_id,
            "quiet_seconds": QUIET_SECONDS,
            "signature": signature,
        })

    watch.pending_report = entry
    last_changed_epoch = watch.pending_report_last_changed_epoch
    if last_changed_epoch is None:
        watch.pending_report_last_changed_epoch = now_epoch
        return None
    if now_epoch - last_changed_epoch < QUIET_SECONDS:
        return None

    due = watch.pending_report
    clear_pending_report(watch)
    warn({
        "thread_id": watch.thread_id,
        "stage": "debounce_emit",
        "mr_id": watch.mr_id,
        "quiet_seconds": QUIET_SECONDS,
        "signature": signature,
    })
    return due


def detect_conflict(watch: MrWatch, mr: dict[str, Any], status: dict[str, Any]) -> dict[str, Any] | None:
    target_branch = watch.target_branch or "master"
    serialized = json.dumps({"mr": mr, "status": status}, ensure_ascii=False).lower()
    conflict_keywords = [
        "conflict",
        "merge conflict",
        "hasconflict",
        "mergeable_state\":\"conflict",
        "rebase_required",
        "cannot_be_merged",
        "冲突",
    ]
    conflict = any(keyword in serialized for keyword in conflict_keywords)
    ready_to_merge = status.get("readyToMerge")
    if not conflict and ready_to_merge is False:
        reason = str(status.get("message") or status.get("mergeStatus") or mr.get("mergeStatus") or "").lower()
        if any(keyword in reason for keyword in ["conflict", "冲突", "rebase"]):
            conflict = True
    if not conflict:
        return None
    summary = f"{watch.branch or '-'} 与 origin/{target_branch} 存在冲突"
    return {
        "target": f"{watch.branch or '-'} -> {target_branch}",
        "summary": summary,
        "action_hint": f"rebase 到 origin/{target_branch} 并解决冲突后重新 push",
    }


def list_ci_runs(watch: MrWatch) -> list[dict[str, Any]]:
    if not watch.branch:
        return []
    data = run_json(
        [
            "a1",
            "-f",
            "json",
            "ci",
            "run",
            "list",
            "--repo",
            watch.repo,
            "--branch",
            watch.branch,
            "--trigger-mode",
            "MR",
            "--per-page",
            "20",
        ]
    )
    if isinstance(data, list):
        return data
    raise RuntimeError(f"unexpected ci run list payload for {watch.repo}@{watch.branch}: {json.dumps(data, ensure_ascii=False)}")


def collect_ci_problems(watch: MrWatch) -> list[dict[str, Any]]:
    if not watch.branch:
        return []
    healthy_statuses = {"SUCCESS", "SUCCEEDED", "PASSED", "OK", "COMPLETE", "RUNNING", "QUEUED", "PENDING", "WAITING", "CREATED"}

    # Idempotency: react to unhealthy runs in the latest MR CI batch. A single
    # push can create several pipeline runs; looking only at the newest run can
    # miss a failed sibling when a later sibling is SUCCESS. The a1 list payload
    # currently has no commit/timestamp, but run ids in the same batch are close
    # together and old batches are separated by a large gap.
    runs = list_ci_runs(watch)

    def _run_sort_key(item: dict[str, Any]) -> tuple[Any, ...]:
        return (
            str(item.get("createdAt") or ""),
            int(item.get("id") or 0),
        )

    runs_sorted = sorted(runs, key=_run_sort_key, reverse=True)
    if not runs_sorted:
        watch.last_ci_signature = None
        return []

    latest_id = int(runs_sorted[0].get("id") or 0)
    latest_revision = str(runs_sorted[0].get("revision") or runs_sorted[0].get("commit") or "")
    if not latest_id:
        return []

    current_batch: list[dict[str, Any]] = []
    for item in runs_sorted:
        try:
            run_id = int(item.get("id") or 0)
        except Exception:
            continue
        if not run_id:
            continue
        if latest_id - run_id > CURRENT_RUN_GROUP_MAX_ID_GAP:
            break
        item_revision = str(item.get("revision") or item.get("commit") or "")
        if latest_revision and item_revision and item_revision != latest_revision:
            continue
        current_batch.append(item)

    issues: list[dict[str, Any]] = []
    signature_parts: list[str] = []
    for item in current_batch:
        run_id = item.get("id")
        status_value = str(item.get("status") or item.get("result") or item.get("state") or "").upper()
        if not run_id or status_value in healthy_statuses:
            continue

        detail: dict[str, Any] = {}
        try:
            detail = run_json(["a1", "-f", "json", "ci", "run", "get", str(run_id), "--repo", watch.repo])
        except Exception:
            detail = {}
        detail_status = str(detail.get("status") or detail.get("result") or detail.get("state") or status_value)
        if detail_status.upper() in healthy_statuses:
            continue

        signature_parts.append(f"{run_id}:{detail_status.upper()}")
        summary = str(detail.get("title") or detail.get("name") or detail.get("pipelineName") or item.get("pipelineName") or item.get("name") or "CI run")
        issues.append(
            {
                "target": f"run#{run_id}",
                "summary": f"{summary} 状态异常：{detail_status}",
                "action_hint": "使用 a1 ci run get / job list / job log 定位失败项并修复；如果 30 分钟后仍未消除，mr-watcher 会补偿重发",
                "status": detail_status,
            }
        )

    if not issues:
        # current batch is healthy/in-flight; reset signature so the next
        # failure transition will emit.
        watch.last_ci_signature = None
        watch.last_ci_reported_at = None
        return []

    signature = "|".join(sorted(signature_parts))
    if signature == watch.last_ci_signature and not compensation_due(watch.last_ci_reported_at):
        return []
    for issue in issues:
        issue["_ci_signature"] = signature
        if signature == watch.last_ci_signature:
            issue["compensation"] = True
            issue["last_reported_at"] = watch.last_ci_reported_at
    # Do not mark last_ci_signature here. Marking before emit can lose a CI
    # transition forever if append_event fails after state is saved.
    watch.last_ci_summary = json.dumps(issues, ensure_ascii=False, sort_keys=True)
    return issues


def collect_status_changes(watch: MrWatch, mr: dict[str, Any], status: dict[str, Any]) -> dict[str, Any]:
    changes: dict[str, Any] = {
        "ci_issues": [],
        "conflict": None,
        "review_required": None,
        "merge_ready": None,
        "merged": False,
        "closed": False,
        "state": None,
        "ready_to_merge": status.get("readyToMerge"),
        "ci_signature": None,
    }

    state = mr.get("state") or status.get("state")
    if state and state != watch.last_state:
        watch.last_state = state
    changes["state"] = state

    test_ok, test_name = bool_from_checks(status, "test")
    approval_ok, approval_name = bool_from_checks(status, "approver_number")
    discussion_ok, discussion_name = bool_from_checks(status, "discussion")
    ready_to_merge = status.get("readyToMerge")

    if test_ok is False:
        changes["ci_issues"] = collect_ci_problems(watch)
        for issue in changes["ci_issues"]:
            if issue.get("_ci_signature"):
                changes["ci_signature"] = issue["_ci_signature"]
                break
    watch.last_test_ok = test_ok

    if approval_ok is False and watch.last_approval_ok is not False:
        changes["review_required"] = {
            "check_name": approval_name or "Require at least 1 reviewer(s) with access to approve",
            "ready_to_merge": ready_to_merge,
        }
    if ready_to_merge is True and watch.last_ready_to_merge is not True:
        changes["merge_ready"] = {
            "ready_to_merge": True,
            "approval_ok": approval_ok,
            "discussion_ok": discussion_ok,
            "test_ok": test_ok,
        }
    watch.last_approval_ok = approval_ok
    watch.last_discussion_ok = discussion_ok
    watch.last_ready_to_merge = ready_to_merge

    conflict = detect_conflict(watch, mr, status)
    if conflict:
        if watch.last_conflict is not True:
            changes["conflict"] = conflict
        watch.last_conflict = True
    else:
        watch.last_conflict = False

    state_norm = str(state or "").lower()
    if state_norm == "merged" and not watch.merged_emitted:
        changes["merged"] = True
        watch.merged_emitted = True

    if state_norm == "closed" and not watch.closed_emitted:
        changes["closed"] = True
        watch.closed_emitted = True
    return changes


def build_delta_entry(watch: MrWatch, new_notes: list[dict[str, Any]], changes: dict[str, Any]) -> dict[str, Any] | None:
    if not new_notes and not any(
        [
            changes.get("ci_issues"),
            changes.get("conflict"),
            changes.get("review_required"),
            changes.get("merge_ready"),
            changes.get("merged"),
            changes.get("closed"),
        ]
    ):
        return None

    targets: list[dict[str, Any]] = []
    if changes.get("conflict"):
        conflict = changes["conflict"]
        targets.append(
            {
                "kind": "conflict",
                "target": conflict["target"],
                "summary": conflict["summary"],
                "action_hint": conflict["action_hint"],
            }
        )

    ci_issues = [
        {k: v for k, v in issue.items() if k != "_ci_signature"}
        for issue in (changes.get("ci_issues") or [])
    ]
    for issue in ci_issues:
        targets.append(
            {
                "kind": "ci",
                "target": issue["target"],
                "summary": issue["summary"],
                "action_hint": issue["action_hint"],
            }
        )

    if changes.get("merge_ready"):
        targets.append(
            {
                "kind": "examiner_result_merge_gate",
                "target": f"mr#{watch.mr_id or watch.mr_url}",
                "summary": "MR 已 readyToMerge=true",
                "action_hint": "router 只通报 human/平台侧可合并；router 没有 merge 权限，不要 handoff delivery。",
                "examiner_result": {"verdict": "merge_ready", "action_target": "router"},
            }
        )

    for comment in new_notes:
        target = f"note#{comment['note_id']}"
        note_kind = "追评" if comment["kind"] == "reply" else "评论"
        reply_to_note = comment.get("root_note_id") or comment["note_id"]
        reply_note_hint = (
            f"该 note 是子评论，Code 平台不支持对子评论继续回复；必须回复第一条根评论 note#{reply_to_note}。"
            if reply_to_note != comment["note_id"]
            else f"回复根评论 note#{reply_to_note}。"
        )
        examiner_result = parse_examiner_result(comment.get("note") or "")
        if examiner_result is not None:
            action_target = examiner_action_target(examiner_result)
            verdict = examiner_result.get("verdict", "unknown")
            severity = examiner_result.get("severity", "unknown")
            art = examiner_result.get("artifact", "")
            if action_target == "delivery":
                kind = "examiner_result_delivery"
                hint = f"审查员 MR 审查 verdict={verdict} severity={severity} art={art}；delivery 按 examiner-review-result findings 逐条修复，push 后 handoff actor_router 请求下一轮 mr_review。"
            elif action_target == "router":
                kind = "examiner_result_router"
                hint = f"审查员 MR 审查要求升级 verdict={verdict} severity={severity} art={art}；router 必须启动 design_review/terminal_review 或 human gate，不要直接交 delivery 继续改。"
            else:
                if verdict == "quality_pass":
                    kind = "examiner_result_merge_gate"
                    hint = f"审查员 MR 审查 verdict={verdict} severity={severity} art={art}；代码质量已通过，router 进入 merge/platform gate。若 readyToMerge=false 仅因非代码 discussion/reviewer/平台项，router 协调 human/平台侧处理；不要唤醒 delivery 修代码。"
                else:
                    kind = "examiner_result_note"
                    hint = f"审查员 MR 审查 verdict={verdict} severity={severity} art={art}；无需唤醒 delivery，继续等待 CI/reviewer/merge gate。"
            targets.append(
                {
                    "kind": kind,
                    "target": target,
                    "summary": f"审查员结果 verdict={verdict} severity={severity} action_target={action_target}",
                    "action_hint": hint,
                    "examiner_result": examiner_result,
                }
            )
            continue

        escalate_to_router = needs_router_escalation(comment.get("note") or "")
        targets.append(
            {
                "kind": "router_escalation" if escalate_to_router else "comment_reply",
                "target": target,
                "summary": f"新增{note_kind}，reviewer={comment['reviewer']}，reply_to_root=note#{reply_to_note}",
                "action_hint": (
                    f"使用 a1 -f json repo mr comment list --repo {watch.repo} --mr {watch.mr_id} 查看原文；该评论包含任务方向/缺陷性质/MR 废弃/需求转化信号，必须 handoff actor_router 发起 design_dispute，由 actor_examiner gate=design_review 裁决；禁止继续交给 delivery 说服式回复。"
                    if escalate_to_router
                    else f"使用 a1 -f json repo mr comment list --repo {watch.repo} --mr {watch.mr_id} 查看原文；{reply_note_hint} 处理后用 a1 repo mr comment create --repo {watch.repo} --mr {watch.mr_id} --reply-to {reply_to_note} -m '<reply>' 回根 note；如果该评论已采纳/已修复，必须继续执行 a1 -f json repo mr comment resolve {reply_to_note} --repo {watch.repo} --mr {watch.mr_id} resolve 根级 inline note，并用 a1 -f json repo mr comment list --repo {watch.repo} --mr {watch.mr_id} --unresolved 复查。只回复不 resolve 不算处理完成；若 note 不是根级 inline 评论或 resolve 失败，必须在 handoff 中说明真实原因。回复内容必须使用中文；若 reviewer 在原则上质疑缺陷/方案（如 API 本身支持、不需要改、AI 瞎修），delivery 必须执行真实 handoff，目标 actor id 写 actor_router，发起 design_dispute，由 actor_examiner gate=design_review 裁决；禁止写旧短 ID router，也不要继续说服式回复。"
                ),
            }
        )

    status_items: list[str] = []
    if changes.get("review_required"):
        status_items.append(f"缺评审通过：{changes['review_required']['check_name']}")
    if changes.get("merge_ready"):
        status_items.append("MR 已通过所有检查：readyToMerge=true，等待 human/平台侧合并。")
    if changes.get("merged"):
        status_items.append("MR 已合并：请 delivery 做 post-merge 收口。")
    if changes.get("closed"):
        status_items.append("MR 已关闭：请 router 接手决定是否重开、放弃或另起任务。")

    return {
        "thread_id": watch.thread_id,
        "repo": watch.repo,
        "mr_url": watch.mr_url,
        "mr_id": watch.mr_id,
        "branch": watch.branch,
        "target_branch": watch.target_branch,
        "state": changes.get("state"),
        "new_notes_count": len(new_notes),
        "new_note_ids": [f"note#{item['note_id']}" for item in new_notes],
        "ci_issues": ci_issues,
        "conflict": changes.get("conflict"),
        "review_required": changes.get("review_required"),
        "merge_ready": changes.get("merge_ready"),
        "merged": changes.get("merged"),
        "closed": changes.get("closed"),
        "ready_to_merge": changes.get("ready_to_merge"),
        "ci_signature": changes.get("ci_signature"),
        "targets": targets,
        "status_items": status_items,
    }


def emit_thread_report(thread_id: str, entries: list[dict[str, Any]]) -> None:
    if not entries:
        return

    lines = ["## MR 扫描报告", "", f"- thread: {thread_id}", f"- touched_mrs: {len(entries)}", ""]
    payload_entries: list[dict[str, Any]] = []
    all_targets: list[dict[str, Any]] = []
    handoff = "actor_delivery"
    event_type = "mr.scan_report"

    has_delivery_work = False
    has_router_work = False
    has_router_only = False
    has_merged_only = False

    for idx, entry in enumerate(entries, start=1):
        lines.extend(
            [
                f"### MR {idx}",
                "",
                f"- repo: {entry['repo']}",
                f"- mr: {entry['mr_url']}",
                f"- branch: {entry['branch'] or '-'} -> {entry['target_branch'] or '-'}",
                f"- state: {entry['state'] or '-'}",
            ]
        )
        if entry["new_notes_count"]:
            note_ids = "、".join(entry["new_note_ids"][:8])
            extra = "" if len(entry["new_note_ids"]) <= 8 else f" 等 {len(entry['new_note_ids'])} 条"
            lines.append(f"- review: 新增 {entry['new_notes_count']} 条评论/追评（{note_ids}{extra}）")
            lines.append(
                f"- how_to_check: a1 -f json repo mr comment list --repo {entry['repo']} --mr {entry['mr_id']}"
            )
            router_targets = [t for t in entry["targets"] if t.get("kind") in {"router_escalation", "examiner_result_router", "examiner_result_merge_gate"}]
            merge_gate_targets = [t for t in router_targets if t.get("kind") == "examiner_result_merge_gate"]
            comment_targets = [t for t in entry["targets"] if t.get("kind") in {"comment_reply", "examiner_result_delivery"}]
            examiner_notes = [t for t in entry["targets"] if t.get("kind") == "examiner_result_note"]
            if router_targets:
                lines.append("- router_actions:")
                for target in router_targets[:8]:
                    lines.append(f"  - {target['target']}: {target.get('summary', '')}")
                    lines.append(f"    action_hint: {target.get('action_hint', '')}")
                has_router_work = True
            if examiner_notes:
                lines.append("- examiner_status:")
                for target in examiner_notes[:8]:
                    lines.append(f'  - {target["target"]}: {target.get("summary", "")}')
                    lines.append(f'    action_hint: {target.get("action_hint", "")}')
            if comment_targets:
                lines.append("- review_actions:")
                for target in comment_targets[:8]:
                    lines.append(f"  - {target['target']}: {target.get('summary', '')}")
                    lines.append(f"    action_hint: {target.get('action_hint', '')}")
                has_delivery_work = True
        for issue in entry["ci_issues"]:
            compensation = "（30 分钟补偿重报）" if issue.get("compensation") else ""
            lines.append(f"- ci: {issue['target']} / {issue['summary']}{compensation}")
            has_delivery_work = True
        if entry["conflict"]:
            lines.append(f"- conflict: {entry['conflict']['summary']}")
            has_delivery_work = True
        for status_item in entry["status_items"]:
            lines.append(f"- status: {status_item}")
        lines.append("")

        all_targets.extend(entry["targets"])
        payload_entries.append(entry)
        if entry["closed"] and not (entry["new_notes_count"] or entry["ci_issues"] or entry["conflict"] or entry["merged"]):
            has_router_only = True
        if entry["merged"] and not (entry["new_notes_count"] or entry["ci_issues"] or entry["conflict"] or entry["closed"] or entry.get("merge_ready")):
            has_merged_only = True

    has_terminal = any(entry["merged"] or entry["closed"] for entry in entries)
    terminal_kind = None
    if has_terminal:
        merged_count = sum(1 for e in entries if e["merged"])
        closed_count = sum(1 for e in entries if e["closed"])
        if merged_count and not closed_count:
            terminal_kind = "merged"
        elif closed_count and not merged_count:
            terminal_kind = "closed"
        else:
            terminal_kind = "mixed"

    if has_terminal:
        handoff = "actor_router"
        event_type = "mr.final"
        guidance = []
        for entry in entries:
            if entry["merged"]:
                guidance.append(f"- {entry['repo']}#{entry['mr_id']} 已合并 → router 请按任务来源收口：如果是 feedback/bugfix 任务，则安排 delivery 回评 feedback 并更新状态；如果是普通对话任务，则只做任务完成确认。")
            elif entry["closed"]:
                guidance.append(f"- {entry['repo']}#{entry['mr_id']} 已关闭/废弃 → router 请判断是否重开、改方案、或直接结束本任务。")
        lines.extend(["### action", "", "- mr-watcher 已停止监听该 MR（终态）。"])
        lines.extend(guidance)
        lines.append("- 本事件只表示当前 MR 终态；是否继续处理队列只能由任务发起方根据本地状态决定。")
    elif has_router_work:
        handoff = "actor_router"
        event_type = "mr.review_escalation"
        merge_gate_only = all(
            t.get("kind") == "examiner_result_merge_gate"
            for entry in entries
            for t in entry.get("targets", [])
            if t.get("kind") in {"router_escalation", "examiner_result_router", "examiner_result_merge_gate"}
        )
        if merge_gate_only:
            event_type = "mr.merge_gate"
            lines.extend(
                [
                    "### action",
                    "",
                    "- router 请进入 merge/platform gate：examiner 已给出 quality_pass，delivery 无需继续改代码。",
                    "- 若 readyToMerge=false 仅因非代码 discussion、reviewer 或平台项，请协调 human/评论方/平台侧处理；不要把该结果再 handoff 给 delivery。",
                    "- 若后续出现新的代码评论、CI 失败或冲突，mr-watcher 会另行唤醒 delivery。",
                ]
            )
        else:
            lines.extend(
                [
                    "### action",
                    "",
                    "- router 请接手判断：该 MR 评论包含 examiner 升级结果或任务方向/缺陷性质/MR 废弃/需求转化信号，不能继续由 delivery 说服式回复。",
                    "- 原则性争议必须启动 actor_examiner gate=design_review；reject/终止类问题进入 terminal_review 或 human gate。",
                ]
            )
    elif has_delivery_work:
        lines.extend(
            [
                "### action",
                "",
                "- delivery 请按 repo/MR 分组自行查询并处理：",
                "  1. 用 a1 repo mr comment list / ci run get / ci job log 查明细",
                "  2. 针对每条 review note，优先遵循上方 review_actions.action_hint；如果 note 是子评论，必须回复根 note，不能回复子评论",
                "  3. 合理的直接修、push，并在 note 下用中文回复 done in <sha>",
                "  4. 不合理或超出 DoD 的也要在根 note 下直接用中文回复说明；原则性质疑必须先 handoff actor_router 发起 design_dispute，由 actor_examiner gate=design_review 裁决，禁止写旧短 ID router",
                "  5. 全部对外输出（MR 评论 / handoff 文本 / 回复内容）一律使用中文；代码、命令、错误堆栈可保留英文。",
            ]
        )

    append_event(
        event_type,
        thread_id,
        "\n".join(lines).rstrip() + "\n",
        handoff=handoff,
        payload={
            "report_title": "MR 扫描报告",
            "entries": payload_entries,
            "targets": all_targets,
            "terminal": has_terminal,
            "terminal_kind": terminal_kind,
        },
        mr_urls=[entry["mr_url"] for entry in entries],
    )


def _thread_sort_key(thread: dict[str, Any]) -> tuple[str, str]:
    created = thread.get("createdAt") or thread.get("created_at") or thread.get("created") or ""
    return (str(created), str(thread.get("id") or ""))


def _merge_watch(current: MrWatch, discovered: MrWatch) -> None:
    current.repo = discovered.repo
    current.mr_url = discovered.mr_url
    current.mr_id = current.mr_id or discovered.mr_id
    current.mr_iid = current.mr_iid or discovered.mr_iid
    current.branch = current.branch or discovered.branch
    current.target_branch = current.target_branch or discovered.target_branch
    current.work_item_id = current.work_item_id or discovered.work_item_id


def _canonicalize_existing_state(
    existing: dict[str, MrWatch],
    terminal: dict[str, dict[str, Any]],
    active_thread_ids: set[str],
) -> dict[str, str]:
    owner_to_key: dict[str, str] = {}
    for key, watch in list(existing.items()):
        if watch.thread_id not in active_thread_ids:
            existing.pop(key, None)
            warn({"thread_id": watch.thread_id, "stage": "drop_inactive_watch", "state_key": key})
            continue
        if terminal_key(watch) in terminal or watch_key(watch) in terminal:
            existing.pop(key, None)
            warn({"thread_id": watch.thread_id, "stage": "drop_terminal_watch", "state_key": key})
            continue
        owner = watch_owner_key(watch)
        keeper_key = owner_to_key.get(owner)
        if keeper_key is not None:
            existing.pop(key, None)
            warn({
                "thread_id": watch.thread_id,
                "stage": "drop_duplicate_owner_watch",
                "state_key": key,
                "owner_key": owner,
                "kept_state_key": keeper_key,
            })
            continue
        owner_to_key[owner] = key
    return owner_to_key


def discover_watches(existing: dict[str, MrWatch], thread_filter: set[str] | None = None) -> dict[str, MrWatch]:
    terminal = load_terminal()
    threads = sorted(list_threads(), key=_thread_sort_key)
    active_thread_ids = {str(thread.get("id")) for thread in threads if thread.get("id")}
    if thread_filter:
        active_thread_ids &= thread_filter
    owner_to_key = _canonicalize_existing_state(existing, terminal, active_thread_ids)

    for thread in threads:
        tid = thread.get("id")
        if not tid:
            continue
        if tid not in active_thread_ids:
            continue
        try:
            events = list_events(tid)
        except Exception as exc:
            warn({"thread_id": tid, "stage": "discover", "error": str(exc)})
            continue
        for discovered in parse_mr_watches(tid, events):
            try:
                discovered = resolve_mr_id(discovered)
            except Exception:
                pass
            key = watch_key(discovered)
            owner = watch_owner_key(discovered)
            if terminal_key(discovered) in terminal or key in terminal:
                continue

            existing_key = owner_to_key.get(owner)
            if existing_key is not None:
                current = existing.get(existing_key)
                if current is not None and current.thread_id != discovered.thread_id:
                    warn({
                        "thread_id": discovered.thread_id,
                        "stage": "duplicate_owner_watch_suppressed",
                        "owner_key": owner,
                        "kept_thread_id": current.thread_id,
                        "kept_state_key": existing_key,
                    })
                    continue
                if current is not None:
                    _merge_watch(current, discovered)
                    if existing_key != key:
                        existing.pop(existing_key, None)
                        existing[key] = current
                        owner_to_key[owner] = key
                    continue

            watch = discovered if discovered.mr_id is not None else resolve_mr_id(discovered)
            key = watch_key(watch)
            owner = watch_owner_key(watch)
            existing_key = owner_to_key.get(owner)
            if existing_key is not None and existing_key != key:
                current = existing.get(existing_key)
                if current is not None and current.thread_id != watch.thread_id:
                    warn({
                        "thread_id": watch.thread_id,
                        "stage": "duplicate_owner_watch_suppressed",
                        "owner_key": owner,
                        "kept_thread_id": current.thread_id,
                        "kept_state_key": existing_key,
                    })
                    continue
            existing[key] = watch
            owner_to_key[owner] = key
    return existing


def poll_once(state: dict[str, MrWatch], thread_filter: set[str] | None = None) -> dict[str, MrWatch]:
    state = discover_watches(state, thread_filter=thread_filter)
    thread_entries: dict[str, list[dict[str, Any]]] = {}
    pending_marks: dict[str, list[tuple[MrWatch, list[dict[str, Any]]]]] = {}
    pending_ci_marks: dict[str, list[tuple[MrWatch, str, list[dict[str, Any]]]]] = {}
    for state_key, watch in list(state.items()):
        if thread_filter and watch.thread_id not in thread_filter:
            continue
        try:
            resolve_mr_id(watch)
            mr = read_mr_view(watch)
            status = read_mr_status(watch)
            comments = list_comments(watch)
            new_comments = collect_new_notes(watch, comments)
            changes = collect_status_changes(watch, mr, status)
            entry = build_delta_entry(watch, new_comments, changes)
            if not entry:
                if watch.pending_report:
                    clear_pending_report(watch)
                continue
            if entry.get("merged") or entry.get("closed"):
                clear_pending_report(watch)
                emitted_entry = entry
            elif has_debounceable_delta(entry):
                emitted_entry = debounced_entry_if_due(watch, entry)
                if not emitted_entry:
                    continue
            else:
                emitted_entry = entry
            thread_entries.setdefault(watch.thread_id, []).append(emitted_entry)
            if emitted_entry.get("new_notes_count") and new_comments:
                pending_marks.setdefault(watch.thread_id, []).append((watch, new_comments))
            if emitted_entry.get("ci_signature"):
                pending_ci_marks.setdefault(watch.thread_id, []).append((watch, emitted_entry["ci_signature"], emitted_entry.get("ci_issues") or []))
        except Exception as exc:
            if should_drop_terminal_state(watch, exc):
                mark_terminal(watch, "missing_or_closed")
                state.pop(state_key, None)
                warn({
                    "thread_id": watch.thread_id,
                    "stage": "drop_missing_or_terminal_mr",
                    "state_key": state_key,
                    "mr_url": watch.mr_url,
                    "mr_id": watch.mr_id,
                    "branch": watch.branch,
                    "error": str(exc),
                })
                continue
            warn({"thread_id": watch.thread_id, "stage": "poll", "state_key": state_key, "error": str(exc)})
    for thread_id, entries in thread_entries.items():
        try:
            emit_thread_report(thread_id, entries)
            for watch, notes in pending_marks.get(thread_id, []):
                try:
                    mark_notes_forwarded(watch, notes)
                except Exception as exc2:
                    warn({"thread_id": thread_id, "stage": "mark_forwarded", "error": str(exc2)})
            for watch, signature, issues in pending_ci_marks.get(thread_id, []):
                watch.last_ci_signature = signature
                watch.last_ci_reported_at = now_iso()
                watch.last_ci_summary = json.dumps(issues, ensure_ascii=False, sort_keys=True)
        except Exception as exc:
            warn({"thread_id": thread_id, "stage": "emit", "error": str(exc)})
            if f"thread {thread_id}" in str(exc):
                for state_key, watch in list(state.items()):
                    if watch.thread_id == thread_id:
                        state.pop(state_key, None)
    for state_key, watch in list(state.items()):
        if watch.merged_emitted or watch.closed_emitted:
            mark_terminal(watch, "merged" if watch.merged_emitted else "closed")
            state.pop(state_key, None)
    save_state(state)
    return state


def loop(thread_ids: set[str] | None = None) -> None:
    state = load_state()
    while True:
        poll_once(state, thread_filter=thread_ids)
        time.sleep(POLL_INTERVAL)


def main() -> int:
    args = parse_args()
    thread_ids = set(args.thread_id or [])
    allow_global = args.allow_global or os.environ.get("MR_WATCHER_ALLOW_GLOBAL") == "1"
    if not thread_ids and not allow_global:
        print(
            json.dumps(
                {
                    "ok": False,
                    "error": "mr-watcher must be started with --thread-id; use --allow-global only for legacy recovery",
                },
                ensure_ascii=False,
            ),
            file=sys.stderr,
        )
        return 2
    try:
        if args.once:
            state = load_state()
            poll_once(state, thread_filter=thread_ids or None)
            return 0
        loop(thread_ids=thread_ids or None)
        return 0
    except KeyboardInterrupt:
        return 0
    except Exception as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, ensure_ascii=False), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
