# 开放多参与者协作协议 Schema 草案 v0

## 1. 文档定位

这份文档定义开放多参与者协作协议的 schema 层。

它不再讨论“对象为什么这样设计”，而只回答四类问题：

1. 方法名是什么。
2. 请求、响应、notification 长什么样。
3. 核心对象的字段如何定义。
4. 不同实现方怎样对接同一套接口。

这层的目标和 [ACP Schema](https://agentclientprotocol.com/protocol/schema) 类似：

- 上层协议先定义清楚接口形状。
- 不同实现方只要按同一套 schema 接入即可互通。
- UI、CLI、MCP、SDK 可以各自有不同 interface，但底层收敛到同一套 wire contract。

这份文档对应的语义层定义，见：

- [open-multi-actor-collaboration-research.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/protocol/open-multi-actor-collaboration-research.md)
- [open-multi-actor-collaboration-protocol-v0.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/protocol/open-multi-actor-collaboration-protocol-v0.md)

## 2. 设计目标

- 稳定：schema 一旦公开，就应该足够稳定，便于不同实现长期接入。
- 清晰：字段职责互斥，不让同一个字段同时承载身份、作用域、连接状态三种含义。
- 可扩展：允许 vendor 扩展，但核心对象和核心方法不被轻易打穿。
- 绑定无关：human UI、agent CLI、agent MCP、SDK 都能映射到同一套 schema。
- 可流式：支持长运行 actor 的增量输出与状态更新。

## 3. 分层

协议应明确区分三层：

### 3.1 语义层

定义对象和行为的含义：

- `Actor`
- `Channel`
- `Thread`
- `Turn`
- `Event`
- `Relation`
- `Artifact`
- `Membership`
- `Delivery`
- `Receipt`
- `Reminder`

### 3.2 Schema 层

定义对外接口：

- 方法名
- 参数
- 返回结果
- notification 结构
- 能力协商结构

### 3.3 Binding 层

定义不同接入方式如何映射到 schema：

- human UI binding
- agent CLI binding
- agent MCP binding
- SDK binding

这意味着：

- 人类用户不需要看到 raw schema。
- Agent 可以通过 CLI、MCP 或 SDK 间接调用 schema。
- 不同 binding 的交互手感可以不同，但提交给 server 的动作必须映射到同一套 schema 方法。
- 文本里的 `@handle` 属于 binding 层输入语法；除非 binding 显式映射为 `hands_off_to`，schema 不为其赋予机器语义。
- Binding 层对同一语义只保留一个 canonical 命名；不要求为了兼容旧拼法继续暴露同义 alias。

## 4. 默认绑定约定

v0 推荐 JSON-RPC 2.0 作为默认 binding 形状，理由是：

- 与 ACP 的生态习惯接近。
- 适合双向流与 request/notification 混合。
- 对 CLI、stdio、WebSocket 都比较友好。

这不是唯一传输方式，但它是 v0 的 canonical schema 形状。

### 4.1 请求

```json
{
  "jsonrpc": "2.0",
  "id": "req_123",
  "method": "scope/read",
  "params": {}
}
```

### 4.2 成功响应

```json
{
  "jsonrpc": "2.0",
  "id": "req_123",
  "result": {}
}
```

### 4.3 错误响应

```json
{
  "jsonrpc": "2.0",
  "id": "req_123",
  "error": {
    "code": -32000,
    "message": "string",
    "data": {}
  }
}
```

### 4.4 Notification

```json
{
  "jsonrpc": "2.0",
  "method": "stream/update",
  "params": {}
}
```

## 5. 通用约定

### 5.1 标识符

- 所有 ID 都是 opaque string。
- 客户端不得从 ID 的格式推断业务含义。
- 同一对象在一个 server authority 内必须保持稳定 ID。

### 5.2 时间

- 时间戳统一使用 RFC 3339 / ISO 8601 UTC 字符串。
- 示例：`2026-04-19T03:20:00Z`

### 5.3 `_meta`

- 所有核心对象和方法参数都允许带 `_meta`。
- `_meta` 用于非标准扩展，不影响核心互操作字段。

示例：

```json
{
  "_meta": {
    "vendor.example/trace_id": "trace_123"
  }
}
```

### 5.4 命名空间

- 核心方法使用不带前缀的固定方法名，如 `scope/read`。
- 扩展方法必须使用 namespaced 形式，如 `vendor.example/custom_method`。
- 扩展事件类型建议使用 namespaced 形式，如 `vendor.example/custom.event`。

## 6. 初始化与能力协商

### 6.1 `initialize`

用途：

- 协商协议版本。
- 协商 server / client capabilities。
- 返回默认 binding 行为。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_init_1",
  "method": "initialize",
  "params": {
    "protocolVersion": "0.1",
    "clientInfo": {
      "name": "example-client",
      "title": "Example Client",
      "version": "1.0.0"
    },
    "clientCapabilities": {
      "stream": { "update": true },
      "scope": { "read": true, "subscribe": true },
      "thread": { "create": true },
      "turn": { "explicit": true },
      "event": { "append": true },
      "message": { "search": true },
      "artifact": { "publish": true, "get": true, "read": true },
      "handoff": { "create": true },
      "reminder": {
        "schedule": true,
        "list": true,
        "cancel": true,
        "snooze": true,
        "update": true
      },
      "receipt": { "record": true }
    }
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_init_1",
  "result": {
    "protocolVersion": "0.1",
    "serverInfo": {
      "name": "collaboration-server",
      "title": "Collaboration Server",
      "version": "0.1.0"
    },
    "serverCapabilities": {
      "stream": { "update": true },
      "scope": { "read": true, "subscribe": true, "unsubscribe": true },
      "thread": { "create": true },
      "turn": { "explicit": true },
      "event": { "append": true },
      "message": { "search": true },
      "artifact": { "publish": true, "get": true, "read": true },
      "handoff": { "create": true },
      "reminder": {
        "schedule": true,
        "list": true,
        "cancel": true,
        "snooze": true,
        "update": true
      },
      "receipt": { "record": true }
    }
  }
}
```

### 6.2 Capability 结构

v0 推荐使用对象型 capability，而不是平铺字符串数组。

原因：

- 便于按对象分组。
- 便于未来细化权限和 profile。
- 比平铺字符串更稳定。

标准 capability 节点：

```json
{
  "stream": { "update": true },
  "scope": { "read": true, "subscribe": true, "unsubscribe": true },
  "thread": { "create": true },
  "turn": { "explicit": true },
  "event": { "append": true },
  "message": { "search": true },
  "artifact": { "publish": true, "get": true, "read": true },
  "handoff": { "create": true },
  "reminder": {
    "schedule": true,
    "list": true,
    "cancel": true,
    "snooze": true,
    "update": true
  },
  "receipt": { "record": true }
}
```

## 7. 核心对象 Schema

### 7.1 `Ref`

```json
{
  "kind": "actor | channel | thread | turn | event | artifact",
  "id": "string",
  "_meta": {}
}
```

### 7.2 `ScopeRef`

```json
{
  "kind": "channel | thread",
  "id": "string",
  "_meta": {}
}
```

### 7.3 `Actor`

```json
{
  "id": "actor_123",
  "kind": "human | agent | service",
  "displayName": "string",
  "capabilities": {},
  "_meta": {}
}
```

### 7.4 `Endpoint`

```json
{
  "id": "endpoint_123",
  "actorId": "actor_123",
  "kind": "gui | cli | mcp | sdk | service",
  "title": "string",
  "_meta": {}
}
```

### 7.5 `Connection`

```json
{
  "id": "conn_123",
  "actorId": "actor_123",
  "endpointId": "endpoint_123",
  "openedAt": "2026-04-19T03:20:00Z",
  "_meta": {}
}
```

### 7.6 `Channel`

```json
{
  "id": "chan_123",
  "title": "string",
  "_meta": {}
}
```

### 7.7 `Thread`

`Thread` 是 channel 公共区某条 event 的讨论分支。`id` 是 server 内部索引；
binding 层的 canonical target 使用 `#<channel_id>:<root_event_id>`。

```json
{
  "id": "thread_123",
  "channelId": "chan_123",
  "title": "string",
  "rootEventId": "evt_001",
  "_meta": {}
}
```

### 7.8 `Turn`

```json
{
  "id": "turn_123",
  "actorId": "actor_123",
  "scope": {
    "kind": "thread",
    "id": "thread_123"
  },
  "triggerEventId": "evt_001",
  "status": "open | closed | failed | cancelled",
  "openedAt": "2026-04-19T03:20:00Z",
  "closedAt": null,
  "_meta": {}
}
```

### 7.9 `Relation`

```json
{
  "kind": "replies_to | hands_off_to | responds_to | attaches_artifact",
  "target": {
    "kind": "event",
    "id": "evt_001"
  },
  "_meta": {}
}
```

### 7.10 `Event`

```json
{
  "id": "evt_123",
  "type": "content.add",
  "actorId": "actor_123",
  "scope": {
    "kind": "thread",
    "id": "thread_123"
  },
  "turnId": "turn_123",
  "seq": 1,
  "occurredAt": "2026-04-19T03:20:01Z",
  "payload": {},
  "relations": [],
  "_meta": {}
}
```

### 7.11 `ArtifactEntry`

```json
{
  "path": "report.md",
  "kind": "file | directory",
  "mediaType": "text/markdown",
  "size": 1024,
  "previewable": true,
  "_meta": {}
}
```

### 7.12 `Artifact`

```json
{
  "id": "art_123",
  "uri": "artifact://authority/art_123",
  "kind": "file | directory",
  "name": "report.md",
  "mediaType": "text/markdown",
  "size": 1024,
  "checksum": "sha256:...",
  "entryCount": 1,
  "entries": [],
  "createdBy": "actor_123",
  "createdAt": "2026-04-19T03:20:01Z",
  "_meta": {}
}
```

### 7.13 `Membership`

```json
{
  "actorId": "actor_456",
  "scope": {
    "kind": "thread",
    "id": "thread_123"
  },
  "joinedAt": "2026-04-19T03:20:00Z",
  "updatedAt": "2026-04-19T03:25:00Z",
  "lastReadEventId": "evt_120",
  "_meta": {}
}
```

### 7.14 `Delivery`

```json
{
  "eventId": "evt_123",
  "actorId": "actor_456",
  "state": "pending | delivered | failed",
  "updatedAt": "2026-04-19T03:20:02Z",
  "_meta": {}
}
```

### 7.15 `Receipt`

```json
{
  "eventId": "evt_123",
  "actorId": "actor_456",
  "kind": "seen | read | accepted | declined | completed",
  "recordedAt": "2026-04-19T03:20:03Z",
  "_meta": {}
}
```

### 7.16 `Reminder`

```json
{
  "id": "rem_123",
  "actorId": "actor_agent_1",
  "title": "follow up",
  "scope": {
    "kind": "thread",
    "id": "thread_123"
  },
  "msgId": "evt_123",
  "fireAt": "2026-04-19T04:20:00Z",
  "repeat": "every:1h",
  "status": "scheduled | fired | cancelled",
  "createdAt": "2026-04-19T03:20:00Z",
  "updatedAt": "2026-04-19T03:20:00Z",
  "lastFiredAt": null,
  "_meta": {}
}
```

### 7.17 `PageInfo`

```json
{
  "hasMore": true,
  "nextCursor": "cursor_123",
  "_meta": {}
}
```

## 8. 核心事件类型

| 类型 | 用途 |
| --- | --- |
| `content.add` | 追加可展示内容 |
| `action.request` | 请求审批、输入或选择 |
| `action.response` | 对 action request 的响应 |
| `artifact.publish` | 记录一次 artifact 发布 |
| `turn.close` | 标记 turn 结束 |

工具调用、内部状态变化、partial 文本 chunk 不再是 `Event`，而是属于该 Turn 的私有 trace，通过 §11.3 的 `turn/trace.update` 通道仅回推给 turn owner。详见语义层 protocol-v0.md §8。

## 9. 方法目录

### 9.1 `connection/open`

用途：

- 建立 actor 在当前 server 上的连接上下文。
- 返回连接信息和默认协作 authority。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_conn_open_1",
  "method": "connection/open",
  "params": {
    "actorId": "actor_123",
    "endpoint": {
      "id": "endpoint_gui_1",
      "actorId": "actor_123",
      "kind": "gui",
      "title": "Desktop UI"
    },
    "_meta": {}
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_conn_open_1",
  "result": {
    "connection": {
      "id": "conn_123",
      "actorId": "actor_123",
      "endpointId": "endpoint_gui_1",
      "openedAt": "2026-04-19T03:20:00Z"
    },
    "_meta": {}
  }
}
```

### 9.2 `connection/close`

用途：

- 显式结束当前连接。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_conn_close_1",
  "method": "connection/close",
  "params": {
    "connectionId": "conn_123"
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_conn_close_1",
  "result": {
    "closed": true
  }
}
```

### 9.3 `scope/subscribe`

用途：

- 把当前连接绑定到 `Channel` 或 `Thread` 的后续实时流。
- 不创建、不删除、不暗示任何 `Membership`。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_scope_sub_1",
  "method": "scope/subscribe",
  "params": {
    "scope": {
      "kind": "thread",
      "id": "thread_123"
    },
    "_meta": {}
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_scope_sub_1",
  "result": {
    "mode": "stream",
    "createdAt": "2026-04-19T03:20:00Z",
    "actorId": "actor_123",
    "scope": {
      "kind": "thread",
      "id": "thread_123"
    }
  }
}
```

### 9.4 `scope/unsubscribe`

用途：

- 取消订阅某个作用域。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_scope_unsub_1",
  "method": "scope/unsubscribe",
  "params": {
    "scope": {
      "kind": "thread",
      "id": "thread_123"
    }
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_scope_unsub_1",
  "result": {
    "unsubscribed": true
  }
}
```

### 9.5 `scope/read`

用途：

- 按作用域读取历史事件。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_scope_read_1",
  "method": "scope/read",
  "params": {
    "scope": {
      "kind": "thread",
      "id": "thread_123"
    },
    "limit": 50,
    "beforeEventId": "evt_999",
    "_meta": {}
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_scope_read_1",
  "result": {
    "events": [],
    "pageInfo": {
      "hasMore": false,
      "nextCursor": null
    }
  }
}
```

### 9.6 `thread/create`

用途：

- 基于某个 `Channel` 公共区中的 `rootEventId` 创建新的 `Thread`。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_thread_create_1",
  "method": "thread/create",
  "params": {
    "channelId": "chan_123",
    "title": "局部收敛",
    "rootEventId": "evt_001",
    "_meta": {}
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_thread_create_1",
  "result": {
    "thread": {
      "id": "thread_123",
      "channelId": "chan_123",
      "title": "局部收敛",
      "rootEventId": "evt_001"
    }
  }
}
```

### 9.7 `turn/open`

用途：

- 显式开启一个 turn。
- 简单消息可以省略，由 server 创建隐式 turn。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_turn_open_1",
  "method": "turn/open",
  "params": {
    "actorId": "actor_agent_1",
    "scope": {
      "kind": "thread",
      "id": "thread_123"
    },
    "triggerEventId": "evt_001",
    "_meta": {}
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_turn_open_1",
  "result": {
    "turn": {
      "id": "turn_123",
      "actorId": "actor_agent_1",
      "scope": {
        "kind": "thread",
        "id": "thread_123"
      },
      "triggerEventId": "evt_001",
      "status": "open",
      "openedAt": "2026-04-19T03:20:00Z",
      "closedAt": null
    }
  }
}
```

### 9.8 `event/append`

用途：

- 向某个 scope 或 turn 追加一条不可变事件。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_event_append_1",
  "method": "event/append",
  "params": {
    "event": {
      "type": "content.add",
      "actorId": "actor_agent_1",
      "scope": {
        "kind": "thread",
        "id": "thread_123"
      },
      "turnId": "turn_123",
      "seq": 1,
      "payload": {
        "contentType": "text/markdown",
        "text": "我先处理第一部分。"
      },
      "relations": [
        {
          "kind": "replies_to",
          "target": {
            "kind": "event",
            "id": "evt_001"
          }
        }
      ],
      "_meta": {}
    }
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_event_append_1",
  "result": {
    "event": {
      "id": "evt_123",
      "type": "content.add",
      "actorId": "actor_agent_1",
      "scope": {
        "kind": "thread",
        "id": "thread_123"
      },
      "turnId": "turn_123",
      "seq": 1,
      "occurredAt": "2026-04-19T03:20:01Z",
      "payload": {
        "contentType": "text/markdown",
        "text": "我先处理第一部分。"
      },
      "relations": []
    }
  }
}
```

### 9.8.1 `message/search`

用途：

- 搜索当前连接 actor 可见的消息文本和标题。
- `scope` 可选；传入时只搜该 scope，未传入时在调用方可见的所有 scope 中搜索。
- 返回按时间倒序排列的 `Event` 列表。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_msg_search_1",
  "method": "message/search",
  "params": {
    "query": "keyword",
    "scope": {
      "kind": "thread",
      "id": "thread_123"
    },
    "limit": 20
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_msg_search_1",
  "result": {
    "events": [
      {
        "id": "evt_123",
        "type": "content.add",
        "actorId": "actor_agent_1",
        "scope": {
          "kind": "thread",
          "id": "thread_123"
        },
        "payload": {
          "contentType": "text/markdown",
          "text": "keyword"
        },
        "relations": []
      }
    ]
  }
}
```

### 9.9 `turn/close`

用途：

- 显式结束一个 turn。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_turn_close_1",
  "method": "turn/close",
  "params": {
    "turnId": "turn_123",
    "status": "closed",
    "_meta": {}
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_turn_close_1",
  "result": {
    "turn": {
      "id": "turn_123",
      "status": "closed",
      "closedAt": "2026-04-19T03:20:02Z"
    }
  }
}
```

### 9.10 `artifact/publish`

用途：

- 发布共享 artifact，返回稳定的 artifact 对象。

artifact 可以在 publish 时带 `scope`，用于把文件落入该 scope 的 workspace
投影；也可以不带 scope，只发布为可寻址共享对象。把 artifact 挂到某个对话动作上，
应通过 `event/append` 携带一条 `content.add` 加上 `attaches_artifact` relation
完成。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_art_publish_1",
  "method": "artifact/publish",
  "params": {
    "ingress": {
      "kind": "inline_text",
      "name": "report.md",
      "mediaType": "text/markdown",
      "text": "# report"
    },
    "createdBy": "actor_agent_1",
    "scope": {
      "kind": "thread",
      "id": "thread_123"
    }
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_art_publish_1",
  "result": {
    "artifact": {
      "id": "art_123",
      "uri": "artifact://authority/art_123",
      "kind": "file",
      "name": "report.md",
      "mediaType": "text/markdown",
      "size": 8,
      "checksum": "sha256:..."
    }
  }
}
```

### 9.11 `artifact/get`

用途：

- 按 `artifactId` 或 `artifact URI` 读取 artifact 元数据。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_art_get_1",
  "method": "artifact/get",
  "params": {
    "artifactUri": "artifact://authority/art_123"
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_art_get_1",
  "result": {
    "artifact": {
      "id": "art_123",
      "uri": "artifact://authority/art_123",
      "kind": "file",
      "name": "report.md",
      "mediaType": "text/markdown",
      "size": 8,
      "checksum": "sha256:..."
    }
  }
}
```

### 9.12 `artifact/read`

用途：

- 从 artifact body 中按 byte offset 读取一段内容；文本预览可用 `content`，
  二进制下载/图片预览可用 `bytes`。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_art_read_1",
  "method": "artifact/read",
  "params": {
    "artifactId": "art_123",
    "offset": 0,
    "maxBytes": 4
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_art_read_1",
  "result": {
    "artifactId": "art_123",
    "mediaType": "text/markdown",
    "offset": 0,
    "truncated": true,
    "nextOffset": 4,
    "content": "# re",
    "bytes": [35, 32, 114, 101]
  }
}
```

说明：

- `offset` 默认 `0`，`maxBytes` 默认 `65536`。
- 当 `truncated` 为 `true` 时，client 应使用 `nextOffset` 发起下一次
  `artifact/read`，直到 `truncated=false`。

### 9.13 `receipt/record`

用途：

- 记录某个 actor 对某个 event 的 receipt。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_receipt_1",
  "method": "receipt/record",
  "params": {
    "eventId": "evt_handoff_1",
    "actorId": "actor_agent_2",
    "kind": "accepted",
    "_meta": {}
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_receipt_1",
  "result": {
    "receipt": {
      "eventId": "evt_handoff_1",
      "actorId": "actor_agent_2",
      "kind": "accepted",
      "recordedAt": "2026-04-19T03:20:04Z"
    }
  }
}
```

### 9.13.1 `delivery/list`

用途：

- 拉取某个 actor 的 durable directed inbox。
- 调用方必须已通过 `connection/open` 绑定到同一个 `actorId`；server 必须拒绝跨 actor
  读取 inbox。
- `message check` 可映射为 `delivery/list(state=pending)` 后对已处理事件
  `receipt/record`。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_delivery_list_1",
  "method": "delivery/list",
  "params": {
    "actorId": "actor_agent_1",
    "state": "pending",
    "limit": 50,
    "cursor": null
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_delivery_list_1",
  "result": {
    "deliveries": [
      {
        "delivery": {
          "eventId": "evt_123",
          "actorId": "actor_agent_1",
          "state": "pending",
          "updatedAt": "2026-04-19T03:20:02Z"
        },
        "event": {}
      }
    ],
    "nextCursor": null
  }
}
```

### 9.14 `reminder/schedule`

用途：

- 创建一个由 `actorId` 拥有的提醒。
- `fireAt` 与 `delaySeconds` 必须至少提供一个；同时提供时以 `fireAt` 为准。
- `scope` 可选；带 scope 的提醒到期时会写入 `reminder.fire` 事件并 directed
  delivery 给 `actorId`。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_rem_schedule_1",
  "method": "reminder/schedule",
  "params": {
    "actorId": "actor_agent_1",
    "title": "follow up",
    "scope": {
      "kind": "thread",
      "id": "thread_123"
    },
    "msgId": "evt_123",
    "delaySeconds": 3600,
    "repeat": "every:1h"
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_rem_schedule_1",
  "result": {
    "reminder": {
      "id": "rem_123",
      "actorId": "actor_agent_1",
      "title": "follow up",
      "fireAt": "2026-04-19T04:20:00Z",
      "status": "scheduled"
    }
  }
}
```

### 9.14.1 `reminder/list`

```json
{
  "jsonrpc": "2.0",
  "id": "req_rem_list_1",
  "method": "reminder/list",
  "params": {
    "actorId": "actor_agent_1",
    "statuses": ["scheduled"],
    "all": false
  }
}
```

响应为 `{ "reminders": [Reminder...] }`。

### 9.14.2 `reminder/cancel`

请求参数：`{ "actorId": "actor_agent_1", "id": "rem_123" }`。
响应为 `{ "reminder": Reminder }`。

### 9.14.3 `reminder/snooze`

请求参数：`{ "actorId": "actor_agent_1", "id": "rem_123", "bySeconds": 600 }`。
响应为 `{ "reminder": Reminder }`。

### 9.14.4 `reminder/update`

请求参数：`actorId`、`id` 必填，`title`、`fireAt`、`delaySeconds`、`repeat` 可选。
响应为 `{ "reminder": Reminder }`。

### 9.15 `turn/trace.read`

用途：

- 拉取某个 Turn 已记录的 trace 帧历史。
- 调用方必须是该 Turn 的 owner（即 `Turn.actorId`）；其它 actor 调用应返回错误。
- trace 不参与 `scope/read`，必须通过该方法显式拉取。

请求：

```json
{
  "jsonrpc": "2.0",
  "id": "req_turn_trace_read_1",
  "method": "turn/trace.read",
  "params": {
    "turnId": "turn_123",
    "limit": 100,
    "beforeSeq": null,
    "_meta": {}
  }
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": "req_turn_trace_read_1",
  "result": {
    "frames": [
      {
        "seq": 1,
        "kind": "tool.start",
        "occurredAt": "2026-04-19T03:20:01Z",
        "payload": {
          "toolName": "search",
          "input": { "query": "..." }
        }
      },
      {
        "seq": 2,
        "kind": "text.delta",
        "occurredAt": "2026-04-19T03:20:01Z",
        "payload": { "text": "正在分析" }
      }
    ],
    "pageInfo": {
      "hasMore": false
    }
  }
}
```

## 10. Artifact Ingress Schema

不同 client interface 可以使用不同的 artifact ingress，但底层都应映射到同一个
`artifact/publish`。当前 v0 实现支持 `inline_text` 与 `file_bytes` 两种入口。

### 10.1 `inline_text`

```json
{
  "kind": "inline_text",
  "name": "report.md",
  "mediaType": "text/markdown",
  "text": "# report",
  "_meta": {}
}
```

### 10.2 `file_bytes`

```json
{
  "kind": "file_bytes",
  "name": "report.md",
  "mediaType": "text/markdown",
  "bytes": [35, 32, 114, 101, 112, 111, 114, 116]
}
```

说明：

- agent CLI 的 `attachment upload` 会把本地文件读成 `file_bytes`。
- agent CLI 的 `artifact publish --text` / `--file` 会映射为 `inline_text`。

## 11. Notification Schema

v0 定义两类 notification：

- `stream/update` —— 面向 scope 订阅者的多 actor 共享事件流，由 `kind` 区分子类型。
- `turn/trace.update` —— 面向单个 Turn owner 的私有 trace 帧推送，**不**参与 scope 广播。

这样做的目的是：

- 公共事件流出口统一，客户端只需要订阅一种更新通道。
- agent 内部活动（工具调用、partial 文本、状态变化、运行时错误）走独立的私有通道，不污染公共事件流，也不消耗其它 actor 的 token。

### 11.1 `stream/update`

```json
{
  "jsonrpc": "2.0",
  "method": "stream/update",
  "params": {
    "kind": "event.created",
    "scope": {
      "kind": "thread",
      "id": "thread_123"
    },
    "data": {},
    "_meta": {}
  }
}
```

标准 `kind`：

- `thread.created`
- `turn.opened`
- `turn.closed`
- `event.created`
- `artifact.published`
- `delivery.updated`
- `receipt.recorded`

### 11.2 `event.created`

```json
{
  "jsonrpc": "2.0",
  "method": "stream/update",
  "params": {
    "kind": "event.created",
    "scope": {
      "kind": "thread",
      "id": "thread_123"
    },
    "data": {
      "event": {
        "id": "evt_123",
        "type": "content.add"
      }
    }
  }
}
```

### 11.3 `artifact.published`

```json
{
  "jsonrpc": "2.0",
  "method": "stream/update",
  "params": {
    "kind": "artifact.published",
    "scope": {
      "kind": "thread",
      "id": "thread_123"
    },
    "data": {
      "artifact": {
        "id": "art_123",
        "uri": "artifact://authority/art_123"
      }
    }
  }
}
```

### 11.4 `turn/trace.update`

`turn/trace.update` 是 server → client 的私有 notification，仅发给当前 Turn 的 owner（`Turn.actorId` 对应的 connection）。

```json
{
  "jsonrpc": "2.0",
  "method": "turn/trace.update",
  "params": {
    "turnId": "turn_123",
    "frame": {
      "seq": 7,
      "kind": "tool.start",
      "occurredAt": "2026-04-19T03:20:01Z",
      "payload": {
        "toolName": "search",
        "input": { "query": "..." }
      }
    },
    "_meta": {}
  }
}
```

标准 `frame.kind`：

- `tool.start` —— agent 开始一次工具调用；payload 含 `toolName` / `input`。
- `tool.update` —— 同一次调用的中间进度；payload 含 ACP adapter 提供的部分输出。
- `tool.end` —— 工具调用结束；payload 含 `status` / `output` / `error`。
- `text.delta` —— agent partial 文本 chunk；payload 含 `text`。
- `status` —— agent 上报的运行时状态变化；payload 含 `status` 字符串。
- `error` —— agent 报告的运行时错误；payload 含 `message`。

约束：

- 服务端不得把 trace 帧广播给除 owner 之外的任何 connection。
- 服务端不得把 trace 帧记录为可寻址的 `Event`（不分配 `eventId`，不能被 `replies_to` / `responds_to` 指向）。
- 重连后 owner 仍可通过 `turn/trace.read` 拉历史。

## 12. 校验规则

### 12.1 Scope 校验

- `Thread.channelId` 必须指向已存在的 `Channel`。
- `Thread.rootEventId` 必须指向 `Thread.channelId` 公共区内已存在的 `Event`。
- `Event.scope` 只能是 `Channel` 或 `Thread`。
- `Turn.scope` 必须与其下 `Event.scope` 保持一致。

### 12.2 `reply` 校验

- `replies_to` 的目标必须是 `Event`。
- `replies_to` 可以形成任意深度的 reply chain。
- `Thread` 不可嵌套。
- `thread/create.rootEventId` 不能指向 thread 内事件。
- 在 `Thread` 内，`replies_to` 的目标必须满足其一：
  - 指向同一个 `Thread` 中的某个 `Event`
  - 指向该 `Thread.rootEventId`

### 12.3 Relation 校验

- `hands_off_to` 的目标必须是 `Actor`。
- `attaches_artifact` 的目标必须是 `Artifact`。
- `responds_to` 的目标必须是 `Event`。

### 12.4 Artifact 校验

- `artifact/publish` 必须提供且仅提供一个 `ingress`。
- server 必须返回稳定 `artifact.id` 与 `artifact.uri`。
- `artifact/read` 只能读取 server 标记为可读的文本内容。

### 12.5 Reminder 校验

- `reminder/schedule` 必须提供非空 `title`。
- `fireAt` 与 `delaySeconds` 至少提供一个；`delaySeconds` 必须为正数。
- 带 `scope` 的 reminder 必须先通过 `actorId` 的 scope ACL 校验。
- `cancel`、`snooze`、`update` 只能操作同一 `actorId` 名下的 reminder。

## 13. Binding Profiles

### 13.1 Human UI Binding

human UI 不需要向用户暴露 raw schema。

推荐映射：

| UI 动作 | Schema 方法 |
| --- | --- |
| 发送消息 | `event/append` |
| 选择 agent 作为下一步处理者 | `event/append (content.add + hands_off_to)` |
| 搜索消息 | `message/search` |
| 新开 thread / 局部讨论 | channel 公共区写 root event 后 `thread/create` |
| 拖拽上传附件 | `artifact/publish` |
| 在消息里附加附件卡片 | `event/append` + `attaches_artifact` |
| 点击接受 handoff | `receipt/record` |
| 创建 / 管理提醒 | `reminder/*` |
| 查看历史 | `scope/read` |
| 订阅实时更新 | `scope/subscribe` |

约束：

- 主 UI 应展示 attachment card、artifact card、preview card。
- 主 UI 不应把 `artifact://...`、opaque ID、raw `_meta` 当作主交互元素。
- raw protocol 信息仅建议在 debug / inspect 视图展示。
- UI 可以把 `@handle` 渲染成高亮文本或引用语法，但这本身不应自动产生机器路由。

### 13.2 Agent CLI Binding

CLI 可以直接暴露接近 schema 的动作。

推荐映射：

| CLI 动作 | Schema 方法 |
| --- | --- |
| `message send --target '#<channel_id>:<root_event_id>'` | `thread/create`（必要时）+ `event/append` |
| `message send --target '#<channel_id>'` | `event/append` |
| `message send --target dm:<actor_id>` | `channel/create` / `channel/invite` / `event/append` |
| `message read --target '#<channel_id>:<root_event_id>'` | `scope/read` |
| `message search --query ...` | `message/search` |
| `message check` | `delivery/list` + `receipt/record` |
| `handoff <actor_id> --in <scope_id>` | `event/append (content.add + hands_off_to)` |
| `attachment upload --target '#<channel_id>:<root_event_id>' --path ...` | `artifact/publish` |
| `artifact publish --name ...` | `artifact/publish` |
| `artifact get ...` | `artifact/get` |
| `artifact read ...` | `artifact/read` |
| `reminder schedule/list/cancel/snooze/update` | `reminder/*` |

CLI target grammar 的 canonical 形式：

- `#<channel_id>`
- `#<channel_id>:<root_event_id>`
- `dm:<actor_id>`

`handoff` 与 `dm:<actor_id>` 是不同语义：前者是当前 scope 内的责任转移 /
唤醒，后者是私聊目标。Binding 不保留 `dm:@actor` 等同义 alias。
`#<channel_id>:<root_event_id>` 中的 root event 必须来自该 channel 公共区；
thread 不能继续套 thread。

### 13.3 Agent MCP Binding

MCP tool 名可以与 schema 方法一一映射。

推荐映射：

| MCP Tool | Schema 方法 |
| --- | --- |
| `thread_create` | `thread/create` |
| `event_append` | `event/append` |
| `message_search` | `message/search` |
| `artifact_publish` | `artifact/publish` |
| `artifact_get` | `artifact/get` |
| `artifact_read` | `artifact/read` |
| `reminder_schedule` / `reminder_list` / `reminder_cancel` / `reminder_snooze` / `reminder_update` | `reminder/*` |
| `receipt_record` | `receipt/record` |

约束：

- tool 名可以为了 MCP 生态改成下划线形式。
- 但参数与返回结果应尽量保持和 schema 对齐。

## 14. 扩展规则

### 14.1 方法扩展

- 扩展方法必须使用 namespaced method name。
- 示例：`vendor.example/custom_method`

### 14.2 对象扩展

- 优先放入 `_meta`
- 不要直接篡改核心字段语义

### 14.3 事件类型扩展

- 推荐使用 namespaced type
- 示例：`vendor.example/custom.event`

### 14.4 关系类型扩展

- 推荐使用 namespaced relation kind
- 示例：`vendor.example/custom_relation`

## 15. 最小兼容要求

一个实现若声称兼容本 schema v0，至少应支持：

- `initialize`
- `connection/open`
- `scope/subscribe`
- `scope/read`
- `thread/create`
- `event/append`
- `message/search`
- `artifact/publish`
- `artifact/get`
- `artifact/read`
- `receipt/record`
- `delivery/list`
- `reminder/schedule`
- `reminder/list`
- `reminder/cancel`
- `reminder/snooze`
- `reminder/update`
- `stream/update`
- `turn/trace.read`
- `turn/trace.update`

并且至少应支持以下对象：

- `Actor`
- `Channel`
- `Thread`
- `Turn`
- `Event`
- `Relation`
- `Artifact`
- `Membership`
- `Delivery`
- `Receipt`
- `Reminder`

## 16. 总结

schema 层只做一件事：

把“不同 client interface 最终都要收敛到同一套接口”这件事，写成稳定、明确、可实现的 contract。

因此：

- 语义层负责定义对象含义。
- schema 层负责定义接口。
- binding 层负责适配不同操作者。

只要这三层不混淆，human UI、agent CLI、agent MCP、SDK、server 都可以独立演进，同时保持互通。
