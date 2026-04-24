#!/usr/bin/env python3
"""
Bridge `am listen --script` bot messages into Joi thread handoffs.

Required env:
  JOI_CHANNEL_ID      Joi channel scope id.
  JOI_TARGET_AGENT    Actor id of the agent to hand off to.

Optional env:
  JOI_SERVER          WebSocket URL for Joi, e.g. ws://127.0.0.1:7878/rpc.
  JOI_SERVICE_ACTOR   Service actor id. Default: svc_am_bridge.
  JOI_SERVICE_DISPLAY Service display name. Default: am bridge.
  JOI_BIN             Joi CLI path. Default: joi.
  JOI_THREAD_ID       Optional fixed Joi thread id. When omitted, the bridge
                      auto-creates/reuses one thread per DingTalk conversation.
  AM_JOI_SCOPE        auto_thread (default), thread, or channel.
  AM_JOI_THREAD_MAP   Mapping file. Default:
                      ~/.config/aone-message-cli/joi-thread-map.json.
  AM_JOI_AUTO_INVITE  If 1, try to invite service + target agent into channel.
  AM_JOI_REPLY        If 1, wait for agent response and reply to DingTalk.
  AM_JOI_REPLY_MODE   callback (default), send, or async_send. callback writes
                      DingTalk stream response JSON to stdout; send calls `am`;
                      async_send returns quickly, then sends with `am` later.
  AM_JOI_SEND_FALLBACK_CHAT
                      If 1, fall back to `am chat <sender>` when group send
                      fails. Default: 1.
  AM_JOI_SEND_PLAIN_TEXT
                      If 1, flatten markdown-ish replies before calling `am`.
                      Default: 1.
  AM_JOI_SEND_MAX_CHARS
                      Max chars sent through `am`. Default: 1800.
  AM_JOI_SEND_ATTEMPTS
                      Max `am` send attempts per command. Default: 4.
  AM_JOI_SEND_RETRY_DELAY_SECONDS
                      Initial retry delay in seconds. Default: 1.
  AM_JOI_SEND_RETRY_BACKOFF
                      Retry delay multiplier. Default: 1.8.
  AM_JOI_REPLY_STRICT If 1, fail the listener callback when `am` reply fails.
  AM_JOI_REPLY_TIMEOUT_SECONDS Default: 120.
  AM_JOI_REPLY_POLL_SECONDS    Default: 2.
  AM_JOI_PENDING_TEXT Text returned immediately in async_send mode.
  AM_BIN              am CLI path. Default: am.
  AM_CONFIG_PATH      am config path. Default:
                      ~/.config/aone-message-cli/config.properties.
"""

import json
import os
import re
import subprocess
import sys
import time
from typing import Any, Iterable, Optional


TEXT_KEYS = {
    "text",
    "content",
    "message",
    "msg",
    "msgcontent",
    "messagecontent",
    "markdown",
    "body",
}
CONVERSATION_KEYS = {
    "conversationid",
    "conversation_id",
    "openconversationid",
    "open_conversation_id",
    "cid",
}
SENDER_KEYS = {
    "staffid",
    "staff_id",
    "senderstaffid",
    "sender_staff_id",
    "senderid",
    "sender_id",
    "fromstaffid",
    "from_staff_id",
    "accountid",
    "account_id",
}
MESSAGE_ID_KEYS = {
    "messageid",
    "message_id",
    "msgid",
    "msg_id",
    "eventid",
    "event_id",
}
BOT_MESSAGE_TOPIC = "/v1.0/im/bot/messages/get"


def die(message: str) -> None:
    print(f"am-joi bridge: {message}", file=sys.stderr)
    raise SystemExit(1)


def env(name: str, default: Optional[str] = None, required: bool = False) -> str:
    value = os.environ.get(name, default)
    if required and not value:
        die(f"missing env {name}")
    return value or ""


def env_bool(name: str, default: bool = False) -> bool:
    value = os.environ.get(name)
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


def maybe_json(value: Any) -> Any:
    if not isinstance(value, str):
        return value
    text = value.strip()
    if not text or text[0] not in "[{":
        return value
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return value


def iter_items(value: Any) -> Iterable[tuple[str, Any]]:
    value = maybe_json(value)
    if isinstance(value, dict):
        for key, child in value.items():
            yield str(key).replace("-", "_").lower(), child
            yield from iter_items(child)
    elif isinstance(value, list):
        for child in value:
            yield from iter_items(child)


def find_field(event: Any, keys: set[str]) -> str:
    for key, value in iter_items(event):
        if key.replace("_", "") in keys or key in keys:
            value = maybe_json(value)
            if isinstance(value, (str, int)):
                return str(value).strip()
    return ""


def extract_text(event: Any, raw: str) -> str:
    for key, value in iter_items(event):
        if key.replace("_", "") not in TEXT_KEYS and key not in TEXT_KEYS:
            continue
        value = maybe_json(value)
        if isinstance(value, dict):
            nested = extract_text(value, "")
            if nested:
                return nested
        if isinstance(value, str) and value.strip():
            return value.strip()
    return raw.strip()


def parse_stdin() -> list[Any]:
    raw = sys.stdin.read()
    if not raw.strip():
        if len(sys.argv) > 1:
            arg_text = " ".join(sys.argv[1:]).strip()
            try:
                parsed = json.loads(arg_text)
                return parsed if isinstance(parsed, list) else [parsed]
            except json.JSONDecodeError:
                return [{"text": arg_text}]
        fallback = os.environ.get("AM_JOI_TEXT")
        return [{"text": fallback}] if fallback else []

    try:
        parsed = json.loads(raw)
        if isinstance(parsed, list):
            return parsed
        return [parsed]
    except json.JSONDecodeError:
        pass

    events = []
    for line in raw.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            events.append({"text": line, "_raw": raw})
    return events


def run(cmd: list[str], check: bool = True) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if check and proc.returncode != 0:
        raise RuntimeError(
            f"command failed ({proc.returncode}): {' '.join(cmd)}\n{proc.stderr.strip()}"
        )
    return proc


def plain_am_text(text: str) -> str:
    lines = []
    for raw_line in text.strip().splitlines():
        line = raw_line.strip()
        if not line:
            continue
        line = re.sub(r"^#{1,6}\s+", "", line)
        line = re.sub(r"^[-*+]\s+", "", line)
        line = re.sub(r"^\d+[.)]\s+", "", line)
        line = line.replace("**", "").replace("__", "").replace("`", "")
        lines.append(line)
    flattened = "；".join(lines)
    flattened = re.sub(r"\s+", " ", flattened).strip()
    flattened = re.sub(r"([:：,，;；.。!！?？])；+", r"\1", flattened)
    max_chars = int(env("AM_JOI_SEND_MAX_CHARS", "1800"))
    if max_chars > 1 and len(flattened) > max_chars:
        flattened = flattened[: max_chars - 1].rstrip() + "…"
    return flattened or text.strip()


def am_staff_id(staff_id: str) -> str:
    if staff_id.isdigit():
        return staff_id.lstrip("0") or staff_id
    return staff_id


class Bridge:
    def __init__(self) -> None:
        self.joi_bin = env("JOI_BIN", "joi")
        self.am_bin = env("AM_BIN", "am")
        self.server = env("JOI_SERVER")
        self.channel_id = env("JOI_CHANNEL_ID", required=True)
        self.thread_id = env("JOI_THREAD_ID")
        self.scope_mode = env("AM_JOI_SCOPE", "auto_thread")
        self.target_agent = env("JOI_TARGET_AGENT", required=True)
        self.service_actor = env("JOI_SERVICE_ACTOR", "svc_am_bridge")
        self.service_display = env("JOI_SERVICE_DISPLAY", "am bridge")
        self.am_config_path = os.path.expanduser(
            env("AM_CONFIG_PATH", "~/.config/aone-message-cli/config.properties")
        )
        self.thread_map = os.path.expanduser(
            env(
                "AM_JOI_THREAD_MAP",
                "~/.config/aone-message-cli/joi-thread-map.json",
            )
        )

    def joi(self, *args: str, json_mode: bool = True) -> list[str]:
        cmd = [self.joi_bin]
        if self.server:
            cmd += ["--server", self.server]
        cmd += ["--as", self.service_actor, "--display", self.service_display]
        if json_mode:
            cmd.append("--json")
        cmd += list(args)
        return cmd

    def am(self, *args: str) -> list[str]:
        cmd = [self.am_bin]
        if self.am_config_path:
            cmd += ["--config-path", self.am_config_path]
        cmd += list(args)
        return cmd

    def send_am(self, cmd: list[str], label: str) -> subprocess.CompletedProcess[str]:
        attempts = max(1, int(env("AM_JOI_SEND_ATTEMPTS", "4")))
        delay = max(0.0, float(env("AM_JOI_SEND_RETRY_DELAY_SECONDS", "1")))
        backoff = max(1.0, float(env("AM_JOI_SEND_RETRY_BACKOFF", "1.8")))
        proc: Optional[subprocess.CompletedProcess[str]] = None
        for attempt in range(1, attempts + 1):
            proc = run(cmd, check=False)
            if proc.returncode == 0:
                if attempt > 1:
                    print(
                        f"am-joi bridge: {label} succeeded on attempt {attempt}/{attempts}",
                        file=sys.stderr,
                    )
                return proc
            detail = proc.stderr.strip() or proc.stdout.strip()
            print(
                f"am-joi bridge: {label} failed attempt {attempt}/{attempts}: {' '.join(cmd)}\n{detail}",
                file=sys.stderr,
            )
            if attempt < attempts and delay > 0:
                time.sleep(delay)
                delay *= backoff
        assert proc is not None
        return proc

    def ensure_actor(self) -> None:
        run(
            self.joi(
                "actor",
                "upsert",
                self.service_actor,
                "--kind",
                "service",
                "--display",
                self.service_display,
            )
        )

    def try_invite(self, actor_id: str) -> None:
        proc = run(
            self.joi("channel", "invite", self.channel_id, actor_id),
            check=False,
        )
        if proc.returncode != 0:
            print(
                f"am-joi bridge: invite skipped for {actor_id}: {proc.stderr.strip()}",
                file=sys.stderr,
            )

    def thread_for(self, source_event: Any) -> str:
        if self.scope_mode == "channel":
            return ""
        if self.thread_id:
            return self.thread_id
        if self.scope_mode not in {"auto_thread", "thread"}:
            raise RuntimeError(
                f"invalid AM_JOI_SCOPE={self.scope_mode!r}; expected auto_thread, thread, or channel"
            )

        key = thread_key(source_event)
        mapping = self.load_thread_map()
        scoped = mapping.setdefault(self.channel_id, {})
        if key in scoped:
            return scoped[key]["threadId"]

        title = thread_title(source_event, key)
        proc = run(self.joi("thread", "create", "--channel", self.channel_id, "--title", title))
        result = json.loads(proc.stdout)
        thread_id = result["thread"]["id"]
        scoped[key] = {
            "threadId": thread_id,
            "title": result["thread"].get("title", title),
            "createdAt": int(time.time()),
        }
        self.save_thread_map(mapping)
        print(
            f"am-joi bridge: created thread {thread_id} for {key}",
            file=sys.stderr,
        )
        return thread_id

    def load_thread_map(self) -> dict[str, Any]:
        try:
            with open(self.thread_map, "r", encoding="utf-8") as f:
                data = json.load(f)
                return data if isinstance(data, dict) else {}
        except FileNotFoundError:
            return {}

    def save_thread_map(self, mapping: dict[str, Any]) -> None:
        parent = os.path.dirname(self.thread_map)
        if parent:
            os.makedirs(parent, exist_ok=True)
        tmp = f"{self.thread_map}.tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(mapping, f, ensure_ascii=False, indent=2, sort_keys=True)
            f.write("\n")
        os.replace(tmp, self.thread_map)

    def handoff(self, source_event: Any, message: str) -> tuple[dict[str, Any], str, str]:
        thread_id = self.thread_for(source_event)
        if self.scope_mode == "channel":
            scope_kind = "channel"
            scope_id = self.channel_id
            scope_args = ["--in", self.channel_id, "--channel"]
        else:
            scope_kind = "thread"
            scope_id = thread_id
            scope_args = ["--in", thread_id]
        proc = run(
            self.joi(
                "handoff",
                self.target_agent,
                *scope_args,
                "--message",
                message,
            )
        )
        return json.loads(proc.stdout), scope_kind, scope_id

    def wait_answer(self, trigger_event_id: str, scope_kind: str, scope_id: str) -> str:
        timeout = float(env("AM_JOI_REPLY_TIMEOUT_SECONDS", "120"))
        interval = float(env("AM_JOI_REPLY_POLL_SECONDS", "2"))
        deadline = time.time() + timeout
        scope_args = ["--in", scope_id]
        if scope_kind == "channel":
            scope_args.append("--channel")
        while time.time() < deadline:
            proc = run(
                self.joi(
                    "event",
                    "list",
                    *scope_args,
                    "--limit",
                    "80",
                )
            )
            result = json.loads(proc.stdout)
            for event in result.get("events", []):
                if event.get("type") != "content.add":
                    continue
                if event.get("actorId") != self.target_agent:
                    continue
                if not responds_to(event, trigger_event_id):
                    continue
                text = event.get("payload", {}).get("text")
                if isinstance(text, str) and text.strip():
                    return text.strip()
            time.sleep(interval)
        raise TimeoutError(f"timed out waiting for response to {trigger_event_id}")

    def reply(self, source_event: Any, answer: str) -> bool:
        mode = env("AM_JOI_REPLY_MODE", "callback").strip().lower()
        if mode == "callback":
            self.reply_callback(source_event, answer)
            return True
        if mode == "send":
            self.reply_am(source_event, answer)
            return False
        if mode == "async_send":
            self.reply_am(source_event, answer)
            return False
        raise RuntimeError(
            f"invalid AM_JOI_REPLY_MODE={mode!r}; expected callback, send, or async_send"
        )

    def reply_callback(self, source_event: Any, answer: str) -> None:
        sender_id = am_staff_id(find_field(source_event, SENDER_KEYS))
        conversation_id = find_field(source_event, CONVERSATION_KEYS)
        msg_param = {"content": answer.strip()}
        if sender_id:
            msg_param["senderStaffId"] = sender_id
        if conversation_id:
            msg_param["conversationId"] = conversation_id
        print(
            json.dumps(
                {
                    "msgKey": "sampleText",
                    "msgParam": json.dumps(msg_param, ensure_ascii=False),
                },
                ensure_ascii=False,
            ),
            flush=True,
        )

    def reply_am(self, source_event: Any, answer: str) -> None:
        conversation_id = find_field(source_event, CONVERSATION_KEYS)
        sender_id = am_staff_id(find_field(source_event, SENDER_KEYS))
        outbound = plain_am_text(answer) if env_bool("AM_JOI_SEND_PLAIN_TEXT", True) else answer.strip()
        if not conversation_id and not sender_id:
            print("am-joi bridge: no am reply target found", file=sys.stderr)
            return

        if env_bool("AM_JOI_DRY_RUN"):
            print(outbound)
            return

        if conversation_id:
            cmd = self.am("group", conversation_id, outbound)
            if sender_id:
                cmd += ["--at", sender_id]
        else:
            cmd = self.am("chat", sender_id, outbound)
        proc = self.send_am(cmd, "am reply")
        if proc.returncode != 0:
            message = (
                f"am reply failed ({proc.returncode}): {' '.join(cmd)}\n"
                f"{proc.stderr.strip()}"
            )
            print(f"am-joi bridge: {message}", file=sys.stderr)
            if conversation_id and sender_id and env_bool("AM_JOI_SEND_FALLBACK_CHAT", True):
                fallback_cmd = self.am("chat", sender_id, outbound)
                fallback_proc = self.send_am(fallback_cmd, "am fallback chat")
                if fallback_proc.returncode == 0:
                    print(
                        f"am-joi bridge: fallback chat sent to {sender_id}",
                        file=sys.stderr,
                    )
                    return
                fallback_message = (
                    f"am fallback chat failed ({fallback_proc.returncode}): {' '.join(fallback_cmd)}\n"
                    f"{fallback_proc.stderr.strip()}"
                )
                print(f"am-joi bridge: {fallback_message}", file=sys.stderr)
                message = message + "\n" + fallback_message
            if env_bool("AM_JOI_REPLY_STRICT"):
                raise RuntimeError(message)


def responds_to(event: dict[str, Any], trigger_event_id: str) -> bool:
    for rel in event.get("relations", []):
        if rel.get("kind") != "responds_to":
            continue
        target = rel.get("target") or {}
        if target.get("kind") == "event" and target.get("id") == trigger_event_id:
            return True
    return False


def format_question(event: Any, text: str) -> str:
    sender = find_field(event, SENDER_KEYS)
    conversation = find_field(event, CONVERSATION_KEYS)
    lines = []
    source = []
    if sender:
        source.append(f"sender={sender}")
    if conversation:
        source.append(f"conversation={conversation}")
    if source:
        lines.append("Source: DingTalk bot message (" + ", ".join(source) + ")")
        lines.append("")
    lines.append("User message:")
    lines.append(text)
    return "\n".join(lines).strip()


def spawn_async_reply(source_event: Any, trigger_id: str, scope_kind: str, scope_id: str) -> None:
    payload = {
        "source_event": source_event,
        "trigger_id": trigger_id,
        "scope_kind": scope_kind,
        "scope_id": scope_id,
    }
    command = [
        sys.executable,
        os.path.abspath(__file__),
        "--async-reply",
        json.dumps(payload, ensure_ascii=False),
    ]
    log_path = os.path.expanduser(
        env("AM_JOI_ASYNC_LOG", "~/.config/aone-message-cli/am-joi-async-reply.log")
    )
    parent = os.path.dirname(log_path)
    if parent:
        os.makedirs(parent, exist_ok=True)
    with open(log_path, "a", encoding="utf-8") as log:
        subprocess.Popen(
            command,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=log,
            start_new_session=True,
        )


def run_async_reply(payload: dict[str, Any]) -> None:
    bridge = Bridge()
    answer = bridge.wait_answer(
        str(payload["trigger_id"]),
        str(payload["scope_kind"]),
        str(payload["scope_id"]),
    )
    bridge.reply_am(payload["source_event"], answer)


def thread_key(event: Any) -> str:
    conversation = find_field(event, CONVERSATION_KEYS)
    if conversation:
        return f"conversation:{conversation}"
    sender = find_field(event, SENDER_KEYS)
    if sender:
        return f"sender:{sender}"
    return "default"


def thread_title(event: Any, key: str) -> str:
    sender = find_field(event, SENDER_KEYS)
    conversation = find_field(event, CONVERSATION_KEYS)
    if conversation:
        label = sender or conversation
        title = f"钉钉答疑 · {label}"
    elif sender:
        title = f"钉钉答疑 · {sender}"
    else:
        title = f"钉钉答疑 · {key}"
    title = re.sub(r"\s+", " ", title).strip()
    return title[:64]


def main() -> None:
    if len(sys.argv) >= 3 and sys.argv[1] == "--async-reply":
        run_async_reply(json.loads(sys.argv[2]))
        return

    bridge = Bridge()
    bridge.ensure_actor()
    if env_bool("AM_JOI_AUTO_INVITE"):
        bridge.try_invite(bridge.service_actor)
        bridge.try_invite(bridge.target_agent)

    wrote_callback_response = False
    for event in parse_stdin():
        raw = event.get("_raw", "") if isinstance(event, dict) else ""
        text = extract_text(event, raw)
        if not text:
            print("am-joi bridge: skipped empty message", file=sys.stderr)
            continue
        result, scope_kind, scope_id = bridge.handoff(event, format_question(event, text))
        trigger_event = result.get("event", {})
        trigger_id = trigger_event.get("id")
        print(
            f"am-joi bridge: handoff event {trigger_id} scope={scope_kind}:{scope_id}",
            file=sys.stderr,
        )
        if env_bool("AM_JOI_REPLY") and trigger_id:
            if env("AM_JOI_REPLY_MODE", "callback").strip().lower() == "async_send":
                spawn_async_reply(event, trigger_id, scope_kind, scope_id)
                bridge.reply_callback(event, env("AM_JOI_PENDING_TEXT", "收到，正在处理。"))
                wrote_callback_response = True
            else:
                answer = bridge.wait_answer(trigger_id, scope_kind, scope_id)
                wrote_callback_response = bridge.reply(event, answer) or wrote_callback_response

    if not wrote_callback_response:
        print("{}", flush=True)


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(f"am-joi bridge: {exc}", file=sys.stderr)
        raise SystemExit(1)
