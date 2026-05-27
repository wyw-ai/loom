#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOOM="${LOOM_BIN:-$ROOT_DIR/target/debug/loom}"
DAEMON="${LOOM_DAEMON_BIN:-$ROOT_DIR/target/debug/loom-daemon}"
SERVER_BIN="${LOOM_SERVER_BIN:-$ROOT_DIR/target/debug/loom-server}"
PORT="${LOOM_E2E_PORT:-17881}"
SERVER="ws://127.0.0.1:${PORT}/rpc"
TMP="${LOOM_E2E_ROOT:-$(mktemp -d /tmp/loom-message-e2e.XXXXXX)}"
LOG="$TMP/server.log"
DAEMON_PID=""

[ -x "$LOOM" ] || { echo "missing $LOOM; run cargo build -p loom-cli -p loom-server" >&2; exit 2; }
[ -x "$DAEMON" ] || { echo "missing $DAEMON; run cargo build -p loom-cli -p loom-server" >&2; exit 2; }
[ -x "$SERVER_BIN" ] || { echo "missing $SERVER_BIN; run cargo build -p loom-cli -p loom-server" >&2; exit 2; }

if "$LOOM" --help | grep -Eq '^[[:space:]]+daemon\b'; then
  echo "loom CLI still exposes daemon subcommand" >&2
  "$LOOM" --help >&2
  exit 1
fi
"$DAEMON" --help | grep -q -- "--machine-id"

"$SERVER_BIN" --bind "127.0.0.1:${PORT}" --data-dir "$TMP/data" >"$LOG" 2>&1 &
SERVER_PID=$!

cleanup() {
  if [ -n "$DAEMON_PID" ]; then
    kill "$DAEMON_PID" >/dev/null 2>&1 || true
    wait "$DAEMON_PID" >/dev/null 2>&1 || true
  fi
  kill "$SERVER_PID" >/dev/null 2>&1 || true
  wait "$SERVER_PID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

for i in $(seq 1 80); do
  if "$LOOM" --server "$SERVER" --as actor_alice --json actor list >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
  if [ "$i" = 80 ]; then
    echo "loom-server did not become reachable" >&2
    cat "$LOG" >&2
    exit 1
  fi
done

json_path='import json,sys; obj=json.load(sys.stdin); path=sys.argv[1].split(".");
for p in path: obj=obj[p]
print(obj)'

LOOM_CONFIG_DIR="$TMP/config" "$DAEMON" --server "$SERVER" \
  --machine-id machine_message_e2e \
  --data-root "$TMP/daemon-data" \
  --allow-actors actor_noop \
  --no-services \
  --no-ipc \
  >"$TMP/daemon.log" 2>&1 &
DAEMON_PID=$!

for i in $(seq 1 80); do
  if grep -q "loom-daemon: ready" "$TMP/daemon.log"; then
    break
  fi
  sleep 0.1
  if [ "$i" = 80 ]; then
    echo "loom-daemon did not become ready" >&2
    cat "$TMP/daemon.log" >&2
    exit 1
  fi
done

for i in $(seq 1 80); do
  actors_json="$("$LOOM" --server "$SERVER" --as actor_alice --json actor list)"
  if printf '%s' "$actors_json" | grep -q "actor_service_machine_message_e2e"; then
    break
  fi
  sleep 0.1
  if [ "$i" = 80 ]; then
    echo "loom-daemon did not register machine actor" >&2
    cat "$TMP/daemon.log" >&2
    printf '%s\n' "$actors_json" >&2
    exit 1
  fi
done

"$LOOM" --server "$SERVER" --as actor_alice actor upsert actor_agent_reviewer --kind agent --json >/dev/null
"$LOOM" --server "$SERVER" --as actor_alice actor upsert actor_bob --kind human --display Bob --json >/dev/null

channel_json="$("$LOOM" --server "$SERVER" --as actor_alice --json channel create --title backend)"
channel_id="$(printf '%s' "$channel_json" | python3 -c "$json_path" channel.id)"
"$LOOM" --server "$SERVER" --as actor_alice channel invite "$channel_id" actor_agent_reviewer --json >/dev/null
"$LOOM" --server "$SERVER" --as actor_alice channel invite "$channel_id" actor_bob --json >/dev/null

agent_config_json="$("$LOOM" --server "$SERVER" --as actor_agent_reviewer --json agent-config publish actor_agent_reviewer --version e2e --model noop --adapter command --tools-json '[]')"
agent_config_id="$(printf '%s' "$agent_config_json" | python3 -c "$json_path" version.id)"
"$LOOM" --server "$SERVER" --as actor_agent_reviewer --json agent-config activate actor_agent_reviewer "$agent_config_id" >/dev/null
run_json="$("$LOOM" --server "$SERVER" --as actor_agent_reviewer --json run open --target "#$channel_id" --start-reason manual --agent-config-version-id "$agent_config_id")"
run_id="$(printf '%s' "$run_json" | python3 -c "$json_path" run.id)"
run_append_json="$("$LOOM" --server "$SERVER" --as actor_agent_reviewer --json run append "$run_id" --status running --frame-kind progress --payload-json '{"phase":"started"}')"
printf '%s' "$run_append_json" | python3 -c 'import json,sys; obj=json.load(sys.stdin); assert obj["frame"]["seq"] == 1, obj'
run_close_json="$("$LOOM" --server "$SERVER" --as actor_agent_reviewer --json run close "$run_id" --status completed)"
printf '%s' "$run_close_json" | python3 -c 'import json,sys; obj=json.load(sys.stdin); assert obj["run"]["status"] == "completed", obj'

root_json="$("$LOOM" --server "$SERVER" --as actor_alice --json message send --target "#$channel_id" --text "root needle")"
root_id="$(printf '%s' "$root_json" | python3 -c "$json_path" message.id)"

reply_json="$("$LOOM" --server "$SERVER" --as actor_alice --json message send --target "#$channel_id:$root_id" --text "thread reply needle")"
reply_id="$(printf '%s' "$reply_json" | python3 -c "$json_path" message.id)"
thread_id="$(printf '%s' "$reply_json" | python3 -c "$json_path" message.scope.id)"
read_json="$("$LOOM" --server "$SERVER" --as actor_alice --json message read --target "#$channel_id:$root_id")"
printf '%s' "$read_json" | grep -q "$reply_id"

"$LOOM" --server "$SERVER" --as actor_bob --json thread follow "$thread_id" >/dev/null
follow_reply_json="$("$LOOM" --server "$SERVER" --as actor_alice --json message send --target "#$channel_id:$root_id" --text "followed thread reply needle")"
follow_reply_id="$(printf '%s' "$follow_reply_json" | python3 -c "$json_path" message.id)"
bob_follow_inbox_json="$("$LOOM" --server "$SERVER" --as actor_bob --json inbox list --no-ack)"
printf '%s' "$bob_follow_inbox_json" | grep -q "$follow_reply_id"
printf '%s' "$bob_follow_inbox_json" | python3 -c 'import json,sys; needle=sys.argv[1]; obj=json.load(sys.stdin); assert any(d["delivery"]["sourceId"] == needle and d["message"]["id"] == needle for d in obj["deliveries"]), obj' "$follow_reply_id"
"$LOOM" --server "$SERVER" --as actor_bob --json inbox list >/dev/null
"$LOOM" --server "$SERVER" --as actor_bob --json thread unfollow "$thread_id" >/dev/null
unfollow_reply_json="$("$LOOM" --server "$SERVER" --as actor_alice --json message send --target "#$channel_id:$root_id" --text "unfollowed thread reply")"
unfollow_reply_id="$(printf '%s' "$unfollow_reply_json" | python3 -c "$json_path" message.id)"
bob_unfollow_inbox_json="$("$LOOM" --server "$SERVER" --as actor_bob --json inbox list --no-ack)"
printf '%s' "$bob_unfollow_inbox_json" | python3 -c 'import json,sys; needle=sys.argv[1]; obj=json.load(sys.stdin); assert all(d["delivery"]["sourceId"] != needle for d in obj["deliveries"]), obj' "$unfollow_reply_id"

task_json="$("$LOOM" --server "$SERVER" --as actor_alice --json task create --source-message "$root_id" --title "Fix needle task" --description "task state shortcuts")"
task_id="$(printf '%s' "$task_json" | python3 -c "$json_path" task.id)"
claim_json="$("$LOOM" --server "$SERVER" --as actor_bob --json task claim "$task_id")"
printf '%s' "$claim_json" | python3 -c 'import json,sys; obj=json.load(sys.stdin); assert obj["task"]["status"] == "claimed", obj; assert obj["task"]["ownerActorId"] == "actor_bob", obj'
complete_json="$("$LOOM" --server "$SERVER" --as actor_bob --json task complete "$task_id" --result "done")"
printf '%s' "$complete_json" | python3 -c 'import json,sys; obj=json.load(sys.stdin); assert obj["task"]["status"] == "done", obj; assert obj["task"]["resultSummary"] == "done", obj'
reopen_json="$("$LOOM" --server "$SERVER" --as actor_alice --json task reopen "$task_id" --owner actor_agent_reviewer)"
printf '%s' "$reopen_json" | python3 -c 'import json,sys; obj=json.load(sys.stdin); assert obj["task"]["status"] == "claimed", obj; assert obj["task"]["ownerActorId"] == "actor_agent_reviewer", obj'
cancel_json="$("$LOOM" --server "$SERVER" --as actor_agent_reviewer --json task cancel "$task_id" --result "not planned")"
printf '%s' "$cancel_json" | python3 -c 'import json,sys; obj=json.load(sys.stdin); assert obj["task"]["status"] == "canceled", obj; assert obj["task"]["resultSummary"] == "not planned", obj'
"$LOOM" --server "$SERVER" --as actor_agent_reviewer --json inbox list >/dev/null
"$LOOM" --server "$SERVER" --as actor_bob --json inbox list >/dev/null

mention_json="$("$LOOM" --server "$SERVER" --as actor_alice --json message send --target "#$channel_id" --text "please inspect @actor_agent_reviewer before merge")"
mention_id="$(printf '%s' "$mention_json" | python3 -c "$json_path" message.id)"
inbox_json="$("$LOOM" --server "$SERVER" --as actor_agent_reviewer --json inbox list --no-ack)"
printf '%s' "$inbox_json" | grep -q "$mention_id"
printf '%s' "$inbox_json" | python3 -c 'import json,sys; needle=sys.argv[1]; obj=json.load(sys.stdin); assert any(d["delivery"]["sourceId"] == needle and d["message"]["id"] == needle for d in obj["deliveries"]), obj' "$mention_id"

"$LOOM" --server "$SERVER" --as actor_agent_reviewer --json inbox list >/dev/null
empty_json="$("$LOOM" --server "$SERVER" --as actor_agent_reviewer --json inbox list --no-ack)"
printf '%s' "$empty_json" | python3 -c 'import json,sys; obj=json.load(sys.stdin); assert obj["deliveries"] == [], obj'

group_json="$("$LOOM" --server "$SERVER" --as actor_alice --json group create --channel "$channel_id" reviewers --member actor_bob)"
group_id="$(printf '%s' "$group_json" | python3 -c "$json_path" group.id)"
"$LOOM" --server "$SERVER" --as actor_alice --json group add-member "$group_id" actor_agent_reviewer >/dev/null
group_list_json="$("$LOOM" --server "$SERVER" --as actor_alice --json group list --channel "$channel_id")"
printf '%s' "$group_list_json" | grep -q "$group_id"
group_message_json="$("$LOOM" --server "$SERVER" --as actor_alice --json message send --target "#$channel_id" --text "ping @reviewers without waking agents")"
group_message_id="$(printf '%s' "$group_message_json" | python3 -c "$json_path" message.id)"
bob_group_inbox_json="$("$LOOM" --server "$SERVER" --as actor_bob --json inbox list --no-ack)"
printf '%s' "$bob_group_inbox_json" | grep -q "$group_message_id"
agent_group_empty_json="$("$LOOM" --server "$SERVER" --as actor_agent_reviewer --json inbox list --no-ack)"
printf '%s' "$agent_group_empty_json" | python3 -c 'import json,sys; obj=json.load(sys.stdin); assert obj["deliveries"] == [], obj'
"$LOOM" --server "$SERVER" --as actor_bob --json inbox list >/dev/null

bots_json="$("$LOOM" --server "$SERVER" --as actor_alice --json group create --channel "$channel_id" bots --member actor_agent_reviewer --wake-agents)"
bots_id="$(printf '%s' "$bots_json" | python3 -c "$json_path" group.id)"
bots_message_json="$("$LOOM" --server "$SERVER" --as actor_alice --json message send --target "#$channel_id" --delivery-policy wake_agent --text "wake @bots")"
bots_message_id="$(printf '%s' "$bots_message_json" | python3 -c "$json_path" message.id)"
bots_inbox_json="$("$LOOM" --server "$SERVER" --as actor_agent_reviewer --json inbox list --no-ack)"
printf '%s' "$bots_inbox_json" | grep -q "$bots_message_id"
"$LOOM" --server "$SERVER" --as actor_agent_reviewer --json inbox list >/dev/null

coord_json="$("$LOOM" --server "$SERVER" --as actor_alice --json coordination propose --target "#$channel_id" --mode sequential --participant actor_bob --participant actor_agent_reviewer --plan-json '{"goal":"count to two"}')"
coord_id="$(printf '%s' "$coord_json" | python3 -c "$json_path" session.id)"
coord_commit_json="$("$LOOM" --server "$SERVER" --as actor_alice --json coordination commit "$coord_id")"
printf '%s' "$coord_commit_json" | python3 -c 'import json,sys; obj=json.load(sys.stdin); assert obj["session"]["batonHolderActorId"] == "actor_bob", obj'
coord_step_a_json="$("$LOOM" --server "$SERVER" --as actor_bob --json coordination step "$coord_id" --base-revision 0 --message "Bob counted one" --output-json '{"count":1}')"
printf '%s' "$coord_step_a_json" | python3 -c 'import json,sys; obj=json.load(sys.stdin); assert obj["session"]["revision"] == 1, obj; assert obj["message"]["body"] == "Bob counted one", obj'
coord_step_b_json="$("$LOOM" --server "$SERVER" --as actor_agent_reviewer --json coordination step "$coord_id" --base-revision 1 --output-json '{"count":2}')"
printf '%s' "$coord_step_b_json" | python3 -c 'import json,sys; obj=json.load(sys.stdin); assert obj["session"]["status"] == "done", obj'

dm_json="$("$LOOM" --server "$SERVER" --as actor_alice --json message send --to actor_agent_reviewer --intent request_action --delivery-policy wake_agent --text "dm needle")"
dm_id="$(printf '%s' "$dm_json" | python3 -c "$json_path" message.id)"
dm_inbox_json="$("$LOOM" --server "$SERVER" --as actor_agent_reviewer --json inbox list --no-ack)"
printf '%s' "$dm_inbox_json" | grep -q "$dm_id"
printf '%s' "$dm_inbox_json" | python3 -c 'import json,sys; needle=sys.argv[1]; obj=json.load(sys.stdin); assert any(d["delivery"]["sourceId"] == needle and d["message"]["id"] == needle for d in obj["deliveries"]), obj' "$dm_id"
"$LOOM" --server "$SERVER" --as actor_agent_reviewer --json inbox list >/dev/null

search_json="$("$LOOM" --server "$SERVER" --as actor_alice --json message search --query needle --target "#$channel_id:$root_id")"
printf '%s' "$search_json" | grep -q "$reply_id"

test -f "$TMP/data/loom.sqlite3"
test ! -f "$TMP/data/journal.jsonl"

echo "loom message e2e ok: root=$root_id reply=$reply_id follow=$follow_reply_id task=$task_id mention=$mention_id group=$group_message_id bots=$bots_id/$bots_message_id run=$run_id coord=$coord_id dm=$dm_id"
