import { LogIn, LogOut, ShieldCheck, UserRound } from "lucide-react";
import { useState } from "react";

import * as ipc from "@/ipc/bridge";
import type { HumanAccount } from "@/ipc/types";
import { AvatarImage } from "@/features/common/ActorAvatar";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { useWorkspaces } from "@/store/workspaces";
import { resetServerScopedStores } from "@/features/workspaces/connect";

export function SettingsPage() {
  const account = useWorkspaces((s) => s.account);

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col bg-white text-black">
      <header className="flex h-panel-header items-center border-b-2 border-black px-5">
        <div className="text-lg font-black">Settings</div>
      </header>
      <div className="stable-scrollbar flex-1 overflow-y-auto p-5">
        <section className="max-w-3xl">
          <div className="mb-3 flex items-center gap-2">
            <UserRound size={18} />
            <h2 className="text-sm font-black uppercase">Account</h2>
          </div>
          {account ? <SignedIn account={account} /> : <SignedOut />}
        </section>
      </div>
    </div>
  );
}

function SignedOut() {
  const [busy, setBusy] = useState(false);

  const login = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const result = await ipc.accountLogin("buc");
      useWorkspaces.getState().setConfig(result.config);
      useSession.getState().setWorkspace(null);
      useSession.getState().setConnection("idle");
      resetServerScopedStores();
      useUI.getState().pushToast("info", "account signed in");
    } catch (err) {
      useUI
        .getState()
        .pushToast(
          "error",
          err instanceof Error ? err.message : String(err),
        );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="card-brutal bg-brutal-cream p-5">
      <div className="mb-4 flex items-start gap-3">
        <div className="btn-brutal-sm h-10 w-10 shrink-0 bg-white">
          <ShieldCheck size={18} />
        </div>
        <div className="min-w-0">
          <div className="font-black">No account connected</div>
          <div className="mt-1 text-xs text-black/55">
            Human identity is required before connecting to a workspace.
          </div>
        </div>
      </div>
      <button
        type="button"
        disabled={busy}
        onClick={login}
        className="btn-brutal gap-2 bg-brutal-cyan px-3 py-2 text-sm"
      >
        <LogIn size={16} />
        {busy ? "Waiting for BUC..." : "Sign in with BUC"}
      </button>
    </div>
  );
}

function SignedIn({ account }: { account: HumanAccount }) {
  const [busy, setBusy] = useState(false);
  const displayName = account.nickname || account.realName || account.staffId;

  const logout = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const cfg = await ipc.accountLogout();
      useWorkspaces.getState().setConfig(cfg);
      useSession.getState().setWorkspace(null);
      useSession.getState().setConnection("idle");
      resetServerScopedStores();
      useUI.getState().pushToast("info", "account signed out");
    } catch (err) {
      useUI
        .getState()
        .pushToast(
          "error",
          err instanceof Error ? err.message : String(err),
        );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="card-brutal bg-white p-5">
      <div className="flex items-start gap-4">
        <AvatarImage
          id={account.actorId}
          label={displayName}
          url={account.avatarUrl}
          size={64}
        />
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <div className="truncate text-lg font-black">{displayName}</div>
            <span className="chip-brutal bg-brutal-lime uppercase">
              {account.provider}
            </span>
          </div>
          {account.realName && account.realName !== displayName && (
            <div className="mt-0.5 text-sm text-black/65">{account.realName}</div>
          )}
          <dl className="mt-4 grid gap-3 text-xs sm:grid-cols-2">
            <Field label="Staff ID" value={account.staffId} mono />
            <Field label="Human ID" value={account.actorId} mono />
            <Field label="Nickname" value={account.nickname || "-"} />
            <Field label="Email" value={account.email || "-"} mono />
          </dl>
        </div>
      </div>
      <div className="mt-5 flex justify-end">
        <button
          type="button"
          disabled={busy}
          onClick={logout}
          className="btn-brutal gap-2 bg-white px-3 py-2 text-sm"
        >
          <LogOut size={16} />
          {busy ? "Signing out..." : "Sign out"}
        </button>
      </div>
    </div>
  );
}

function Field({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="min-w-0 border-l-2 border-black pl-3">
      <dt className="text-[10px] font-black uppercase text-black/45">{label}</dt>
      <dd className={mono ? "truncate font-mono" : "truncate"}>{value}</dd>
    </div>
  );
}
