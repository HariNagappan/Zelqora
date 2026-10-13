// Activity & command history (design Screen 4): what Nova heard, understood and did.

import { useCallback, useEffect, useRef, useState } from "react";

import { Icon } from "../components/Icon";
import { Banner, Button, Card, EmptyState, SearchField, Skeleton } from "../components/ui";
import { confirmAction } from "../components/Dialog";
import { api } from "../services/tauri";
import { useNovaStore } from "../stores/useNova";
import { toastError, toastInfo, toastSuccess } from "../stores/useToasts";
import { formatActivityTime, formatRelative } from "../lib/format";
import { OFFLINE_COPY, useOnline } from "../lib/online";
import type { ActivityEntry } from "../types";

type StatusFilter = "all" | "success" | "failed" | "cancelled";

export default function Activity() {
  const activityBump = useNovaStore((store) => store.activityBump);
  const setManualInput = useNovaStore((store) => store.setManualInput);
  const [entries, setEntries] = useState<ActivityEntry[] | null>(null);
  const [counts, setCounts] = useState<[number, number, number]>([0, 0, 0]);
  const [filter, setFilter] = useState<StatusFilter>("all");
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [retrying, setRetrying] = useState<number | null>(null);
  const online = useOnline();
  const bumpRef = useRef(activityBump);

  const load = useCallback(async () => {
    try {
      const [rows, summary] = await Promise.all([
        api.listActivityView({ status: filter, query, limit: 200 }),
        api.getActivityCounts(),
      ]);
      setEntries(rows);
      setCounts(summary);
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  }, [filter, query]);

  useEffect(() => {
    void load();
  }, [load]);

  // A new command was interpreted — refresh without waiting for a poll.
  useEffect(() => {
    if (bumpRef.current !== activityBump) {
      bumpRef.current = activityBump;
      void load();
    }
  }, [activityBump, load]);

  const [total, success, cancelled] = counts;
  const failed = Math.max(0, total - success - cancelled);

  const filterChip = (id: StatusFilter, label: string, count: number) => (
    <button
      key={id}
      className={`fchip${filter === id ? " on" : ""}`}
      aria-pressed={filter === id}
      onClick={() => setFilter(id)}
    >
      {label} <span className="n">{count}</span>
    </button>
  );

  const retry = async (entry: ActivityEntry) => {
    setRetrying(entry.id);
    try {
      await api.retryActivity(entry.id);
      toastInfo("Retrying", "Nova is running that command again.");
    } catch (err) {
      toastError("Couldn't retry that command", String(err));
    } finally {
      setRetrying(null);
    }
  };

  const clearHistory = async () => {
    confirmAction({
      title: "Clear command history?",
      icon: "trash",
      destructive: true,
      confirmLabel: "Clear history",
      body: (
        <p>
          All transcripts, intents and results will be removed from this device. Your tasks are not
          affected.
        </p>
      ),
      onConfirm: async () => {
        const removed = await api.clearActivityHistory();
        toastSuccess("History cleared", `${removed} entries removed.`);
        void load();
      },
    });
  };

  return (
    <>
      <div className="ph">
        <div>
          <h1>Activity</h1>
          <p className="mut">What Nova heard, understood, and did</p>
        </div>
        {total > 0 ? (
          <Button variant="gst" icon="trash" onClick={() => void clearHistory()}>
            Clear history
          </Button>
        ) : null}
      </div>

      {!online ? <Banner kind="wn">{OFFLINE_COPY}</Banner> : null}

      {error ? (
        <Banner kind="er">
          Nova couldn’t read the command history. <span className="dim">{error}</span>
          <button
            className="more"
            style={{ marginLeft: 8, textDecoration: "underline" }}
            onClick={() => void load()}
          >
            Retry
          </button>
        </Banner>
      ) : null}

      <div className="tools">
        <SearchField
          value={query}
          onChange={setQuery}
          placeholder="Search commands"
          label="Search commands"
        />
        <div className="fchips" role="group" aria-label="Filter commands">
          {filterChip("all", "All", total)}
          {filterChip("success", "Success", success)}
          {filterChip("failed", "Failed", failed)}
          {filterChip("cancelled", "Cancelled", cancelled)}
        </div>
      </div>

      {entries === null ? (
        <div>
          {[0, 1, 2].map((row) => (
            <Card key={row}>
              <div className="arh">
                <Skeleton width={40} height={12} />
                <Skeleton width="60%" height={13} />
                <Skeleton width={90} height={18} />
              </div>
            </Card>
          ))}
        </div>
      ) : entries.length === 0 ? (
        <Card>
          <div className="cbd" style={{ paddingTop: 16 }}>
            <EmptyState
              icon={total > 0 ? "search" : "mic"}
              title={total > 0 ? "No commands match" : "Nothing here yet"}
              detail={
                total > 0
                  ? "Try another filter or search term."
                  : "Say “Hey Nova, add buy milk to my to-dos” — every command appears here with what Nova heard, understood, and did."
              }
              action={
                total > 0 ? (
                  <Button
                    variant="sec"
                    icon="search"
                    onClick={() => {
                      setFilter("all");
                      setQuery("");
                    }}
                  >
                    Clear filters
                  </Button>
                ) : (
                  <Button
                    variant="sec"
                    icon="mic"
                    onClick={() => {
                      setManualInput(true);
                      void api.showFloatingAssistant();
                    }}
                  >
                    Try a command
                  </Button>
                )
              }
            />
          </div>
        </Card>
      ) : (
        entries.map((entry) => {
          const status = statusBadge(entry);
          const open = expanded === entry.id;
          return (
            <div className={`ar${open ? " open" : ""}`} key={entry.id} data-aid={entry.id}>
              <button
                className="arh"
                aria-expanded={open}
                onClick={() => setExpanded(open ? null : entry.id)}
              >
                <span className="tm" title={formatRelative(entry.timestamp)}>
                  {formatActivityTime(entry.timestamp)}
                </span>
                <span className="q">{entry.heard || entry.transcript || entry.response}</span>
                {entry.intent_label ? <span className="bdg mut">{entry.intent_label}</span> : null}
                <span className={`bdg ${status.kind}`}>
                  <Icon name={status.icon} />
                  {status.label}
                </span>
                <span className="cki">
                  <Icon name="ckd" />
                </span>
              </button>
              <div className="arb">
                <div className="dgrid">
                  <span className="dk">Heard</span>
                  <span>“{entry.heard || entry.transcript || "—"}”</span>
                  <span className="dk">Understood</span>
                  <span>{entry.understood ?? entry.intent ?? "No command matched"}</span>
                  <span className="dk">Action</span>
                  <span>{entry.action ?? "Nothing changed."}</span>
                  <span className="dk">Result</span>
                  <span>{entry.response}</span>
                  {entry.duration_ms != null ? (
                    <>
                      <span className="dk">Took</span>
                      <span>{entry.duration_ms} ms</span>
                    </>
                  ) : null}
                </div>
                {entry.retryable || status.kind === "er" ? (
                  <div className="row" style={{ gap: 8 }}>
                    {entry.retryable ? (
                      <Button
                        variant="sec"
                        small
                        icon="refresh"
                        disabled={retrying === entry.id}
                        onClick={() => void retry(entry)}
                      >
                        {retrying === entry.id ? "Retrying…" : "Retry"}
                      </Button>
                    ) : null}
                    <Button variant="gst" small onClick={() => void api.openMain("settings")}>
                      Open settings
                    </Button>
                  </div>
                ) : null}
              </div>
            </div>
          );
        })
      )}
    </>
  );
}

function statusBadge(entry: ActivityEntry): {
  kind: "ok" | "er" | "wn" | "mut";
  label: string;
  icon: "check" | "alert" | "x" | "clock";
} {
  switch (entry.status) {
    case "success":
      return { kind: "ok", label: "Success", icon: "check" };
    case "cancelled":
      return { kind: "wn", label: "Cancelled", icon: "x" };
    case "awaiting_confirmation":
      return { kind: "mut", label: "Awaiting confirmation", icon: "clock" };
    case "awaiting_clarification":
      return { kind: "mut", label: "Needs an answer", icon: "clock" };
    case "timeout":
      return { kind: "er", label: "Timed out", icon: "clock" };
    case "unsupported":
      return { kind: "er", label: "Not supported", icon: "alert" };
    default:
      return { kind: "er", label: "Failed", icon: "alert" };
  }
}
