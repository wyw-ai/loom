#!/usr/bin/env bash
# MR_DETECTOR_FETCH_CMD-compatible: prints canonicalised "open, ci=success" payload.
# Args: $1 = mr_url (ignored — fixture is single-tenant).
cat <<'JSON'
{"title":"feat: e2e fixture MR","head_sha":"deadbeef","base_sha":"cafef00d","author":"e2e","labels":["ready-for-review"],"ci_status":"success","state":"open"}
JSON
