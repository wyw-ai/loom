import { useEffect, useMemo, useState } from "react";
import clsx from "clsx";

import * as ipc from "@/ipc/bridge";
import type { Actor } from "@/ipc/types";

import { PixelAvatar } from "./PixelAvatar";

const resolvedAvatarUrls = new Map<string, string | Promise<string>>();

export function ActorAvatar({
  actor,
  id,
  label,
  size = 36,
  className,
}: {
  actor?: Actor;
  id?: string;
  label?: string;
  size?: number;
  className?: string;
}) {
  const actorId = id ?? actor?.id ?? "actor";
  const display = label ?? actor?.displayName ?? actorId;

  return (
    <AvatarImage
      id={actorId}
      label={display}
      url={avatarUrlForActor(actor)}
      size={size}
      className={className}
    />
  );
}

export function AvatarImage({
  id,
  label,
  url,
  size = 36,
  className,
}: {
  id: string;
  label?: string;
  url?: string | null;
  size?: number;
  className?: string;
}) {
  const normalizedUrl = useMemo(() => normalizeAvatarUrl(url), [url]);
  const src = useCachedAvatarUrl(normalizedUrl);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setFailed(false);
  }, [src]);

  if (!src || failed) {
    return (
      <PixelAvatar id={id} label={label} size={size} className={className} />
    );
  }

  return (
    <img
      src={src}
      alt={label ?? id}
      title={label ?? id}
      width={size}
      height={size}
      className={clsx(
        "inline-block shrink-0 border-2 border-black bg-brutal-cream object-cover",
        className,
      )}
      style={{ width: size, height: size }}
      onError={() => setFailed(true)}
    />
  );
}

function useCachedAvatarUrl(url: string | null) {
  const [src, setSrc] = useState<string | null>(() => {
    if (!url?.startsWith("data:")) return null;
    return url;
  });

  useEffect(() => {
    if (!url) {
      setSrc(null);
      return;
    }
    if (url.startsWith("data:")) {
      setSrc(url);
      return;
    }

    let alive = true;
    const cached = resolvedAvatarUrls.get(url);
    if (typeof cached === "string") {
      setSrc(cached);
      return;
    }
    if (cached) {
      cached.then((value) => {
        if (alive) setSrc(value);
      });
      return () => {
        alive = false;
      };
    }

    setSrc(null);
    const pending = ipc.avatarCachedUrl(url).catch(() => url);
    resolvedAvatarUrls.set(url, pending);
    pending.then((value) => {
      resolvedAvatarUrls.set(url, value);
      if (alive) setSrc(value);
    });

    return () => {
      alive = false;
    };
  }, [url]);

  return src;
}

function avatarUrlForActor(actor?: Actor): string | null {
  const meta = actor?._meta;
  const direct = stringField(meta?.avatarUrl);
  if (direct) return direct;

  const account = recordField(meta?.account);
  const staffId = stringField(account?.staffId);
  if (staffId) {
    return `//work.alibaba-inc.com/photo/${staffId}.140x140.jpg`;
  }
  return null;
}

function normalizeAvatarUrl(url?: string | null): string | null {
  const trimmed = url?.trim();
  if (!trimmed) return null;
  if (trimmed.startsWith("data:")) return trimmed;
  if (trimmed.startsWith("//")) return `https:${trimmed}`;
  if (trimmed.startsWith("http://") || trimmed.startsWith("https://")) {
    return trimmed;
  }
  return null;
}

function stringField(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function recordField(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object"
    ? (value as Record<string, unknown>)
    : null;
}
