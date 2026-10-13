// Home dashboard (design Screen 2).

import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";

import TaskRow from "../components/TaskRow";
import TaskEditor from "../components/TaskEditor";
import { Icon } from "../components/Icon";
import { Badge, Banner, Button, Card, CardHead, Dot, EmptyState, Skeleton, SkeletonTasks } from "../components/ui";
import { confirmAction } from "../components/Dialog";
import { api } from "../services/tauri";
import { useNovaStore } from "../stores/useNova";
import { useOnboardingStore } from "../stores/useOnboarding";
import { useStatusStore } from "../stores/useStatus";
import { useTaskStore } from "../stores/useTasks";
import { toastInfo, toastSuccess } from "../stores/useToasts";
import { formatActivityTime, greeting } from "../lib/format";
import { OFFLINE_COPY, useOnline } from "../lib/online";
import type { ActivityEntry, Todo } from "../types";

export default function Home() {
  const navigate = useNavigate();
  const status = useStatusStore((store) => store.status);
  const nova = useNovaStore();
  const tasks = useTaskStore();
  const openOnboarding = useOnboardingStore((store) => store.setOpen);
  const online = useOnline();
  const [recent, setRecent] = useState<ActivityEntry[] | null>(null);
  const [recentError, setRecentError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Todo | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);

  const loadRecent = useCallback(async () => {
    try {
      const rows = await api.listActivityView({ limit: 3 });
      setRecent(rows);
      setRecentError(null);
    } catch (error) {
      setRecentError(String(error));
    }
  }, []);

  useEffect(() => {
    void loadRecent();
    void tasks.load({ silent: true });
  }, [loadRecent, nova.activityBump]);

  const todayPending = tasks.tasks.filter((task) => !task.completed && isToday(task));
  const recentlyDone = tasks.tasks.filter((task) => task.completed).slice(0, 2);
  const todayEmpty = todayPending.length === 0 && recentlyDone.length === 0;

  const serviceLabel =
    !status
      ? "checking Nova’s status…"
      : nova.service === "running"
        ? "Nova is listening in the background"
        : nova.service === "paused"
          ? "Nova isn't listening right now"
          : "Nova is stopped";

  const removeTask = (task: Todo) => {
    confirmAction({
      title: "Delete this task?",
      icon: "trash",
      destructive: true,
      confirmLabel: "Delete task",
      body: (
        <p>
          “{task.title}” will be removed from your list. This can’t be undone.
        </p>
      ),
      onConfirm: async () => {
        const removed = await tasks.remove(task.id);
        if (removed) {
          toastSuccess("Task deleted", `“${removed.title}” was removed.`);
        }
      },
    });
  };

  return (
    <>
      <div className="ph">
        <div>
          <h1>{greeting()}</h1>
          <p className="mut">
            {tasks.counts.pending} pending · {tasks.counts.overdue} overdue · {serviceLabel}
          </p>
        </div>
        <Button
          variant="sec"
          icon="exp"
          onClick={() => {
            void api.showFloatingAssistant();
            toastInfo("Floating assistant", "It stays on top of your other windows.");
          }}
        >
          Show floating assistant
        </Button>
      </div>

      {!online ? <Banner kind="wn">{OFFLINE_COPY}</Banner> : null}
      {tasks.error ? (
        <Banner kind="er">
          Nova couldn’t load your tasks. <span className="dim">{tasks.error}</span>
          <button
            className="more"
            style={{ marginLeft: 8, textDecoration: "underline" }}
            onClick={() => void tasks.load()}
          >
            Retry
          </button>
        </Banner>
      ) : null}

      <div className="grid2">
        <Card style={{ margin: 0 }}>
          <CardHead>
            <Dot
              kind={
                !status
                  ? "mut"
                  : nova.service === "running" && nova.microphone.available
                    ? "ok"
                    : nova.service === "paused"
                      ? "wn"
                      : "mut"
              }
              live={Boolean(status) && nova.service === "running" && nova.microphone.available}
            />
            <h2>Assistant status</h2>
            <span className="grow" />
            {!status ? (
              <Badge kind="mut">Checking…</Badge>
            ) : !nova.microphone.available ? (
              <Badge kind="er" icon="micoff">
                Mic unavailable
              </Badge>
            ) : nova.service === "running" ? (
              <Badge kind="ok" icon="check">
                Running
              </Badge>
            ) : nova.service === "paused" ? (
              <Badge kind="wn">Paused</Badge>
            ) : (
              <Badge kind="mut">Stopped</Badge>
            )}
          </CardHead>
          <div className="cbd">
            <div className="kv">
              <span className="k">Wake word</span>
              <span className="v">
                “{status?.wakeWord ?? "hey nova"}” ·{" "}
                {!status
                  ? "checking…"
                  : nova.service === "running" && status.wakeEnabled
                    ? "enabled"
                    : "paused"}
              </span>
            </div>
            <div className="kv">
              <span className="k">Microphone</span>
              <span className="v">{nova.microphone.device ?? "System default"}</span>
            </div>
            <div className="kv">
              <span className="k">Speech</span>
              <span className="v">
                <Icon name="cpu" />
                {!status
                  ? "checking…"
                  : status.sttProvider === "cloud"
                    ? "Cloud (command only)"
                    : "On-device"}
              </span>
            </div>
            <div className="kv">
              <span className="k">Understanding</span>
              <span className="v">
                {!status
                  ? "checking…"
                  : status.llmConfigured
                    ? "Cloud model + offline rules"
                    : "Offline rules"}
              </span>
            </div>
            {status && !status.modelReady ? (
              <Banner kind="wn">
                The local speech model isn’t installed, so voice input is off. Typed commands and
                the dashboard still work — install it from Settings → Speech &amp; AI.
              </Banner>
            ) : null}
            <div className="row" style={{ gap: 8, marginTop: 6 }}>
              <Button
                variant="pri"
                icon="mic"
                onClick={() => {
                  void api.activate();
                  toastInfo("Listening", "Say your command now.");
                }}
              >
                Test voice activation
              </Button>
              <Button variant="sec" icon="set" onClick={() => navigate("/settings")}>
                Voice settings
              </Button>
            </div>
          </div>
        </Card>

        <Card style={{ margin: 0 }}>
          <CardHead icon="spark">
            <h2>New here?</h2>
          </CardHead>
          <div className="cbd">
            <p className="mut" style={{ fontSize: 13 }}>
              Run the two-minute setup — test your microphone and say your first command.
            </p>
            <Button
              variant="sec"
              style={{ alignSelf: "flex-start" }}
              iconRight="ckr"
              onClick={() => openOnboarding(true)}
            >
              Start setup
            </Button>
          </div>
        </Card>
      </div>

      <Card>
        <CardHead>
          <h2>Today</h2>
          <Badge kind="mut">{tasks.today.pendingToday} pending</Badge>
          <Badge kind="ok" icon="check">
            {tasks.today.completedToday} done
          </Badge>
          <span className="grow" />
          <button className="more" onClick={() => navigate("/tasks")}>
            Open Tasks <Icon name="ckr" size={12} />
          </button>
        </CardHead>
        <div className="cbd">
          {tasks.loading ? (
            <SkeletonTasks rows={2} />
          ) : todayEmpty ? (
            <EmptyState
              icon="check"
              title={tasks.counts.all === 0 ? "All clear" : "Nothing due today"}
              detail={
                tasks.counts.all === 0
                  ? "Say “Hey Nova, add …” to capture something new."
                  : "Nothing is due today — your other tasks are on the Tasks page."
              }
              action={
                <Button variant="sec" icon="plus" onClick={() => setEditorOpen(true)}>
                  Add a task
                </Button>
              }
            />
          ) : (
            <div className="tlst">
              {todayPending.map((task) => (
                <TaskRow
                  key={task.id}
                  task={task}
                  onToggle={(next) => void tasks.toggle(next.id, !next.completed)}
                  onEdit={(next) => {
                    setEditing(next);
                    setEditorOpen(true);
                  }}
                  onDelete={removeTask}
                />
              ))}
              {recentlyDone.map((task) => (
                <TaskRow
                  key={task.id}
                  task={task}
                  onToggle={(next) => void tasks.toggle(next.id, !next.completed)}
                  onDelete={removeTask}
                />
              ))}
            </div>
          )}
        </div>
      </Card>

      <Card>
        <CardHead>
          <h2>Recent voice commands</h2>
          <span className="grow" />
          <button className="more" onClick={() => navigate("/activity")}>
            View all <Icon name="ckr" size={12} />
          </button>
        </CardHead>
        <div className="cbd">
          {recentError ? (
            <Banner kind="er">
              Couldn’t load your recent commands. <button className="more" style={{ textDecoration: "underline" }} onClick={() => void loadRecent()}>Retry</button>
            </Banner>
          ) : recent === null ? (
            <>
              <Skeleton width="100%" height={18} />
              <Skeleton width="80%" height={18} />
            </>
          ) : recent.length === 0 ? (
            <p className="mut" style={{ fontSize: 13 }}>
              No commands yet — try the floating assistant or say “Hey Nova, add buy milk to my
              to-dos.”
            </p>
          ) : (
            recent.map((entry) => (
              <button
                key={entry.id}
                className="arh"
                style={{ borderRadius: 6 }}
                onClick={() => navigate("/activity")}
              >
                <span className="tm">{formatActivityTime(entry.timestamp)}</span>
                <span className="q">{entry.heard || entry.transcript || entry.response}</span>
                <span
                  className={`bdg ${
                    entry.success ? "ok" : entry.status === "cancelled" ? "wn" : "er"
                  }`}
                >
                  {entry.success ? "Success" : entry.status === "cancelled" ? "Cancelled" : "Failed"}
                </span>
              </button>
            ))
          )}
        </div>
      </Card>

      {editorOpen ? (
        <TaskEditor
          task={editing}
          onClose={() => {
            setEditorOpen(false);
            setEditing(null);
          }}
        />
      ) : null}
    </>
  );
}

function isToday(task: Todo): boolean {
  if (!task.due_at) {
    return false;
  }
  const due = new Date(task.due_at);
  if (Number.isNaN(due.getTime())) {
    return false;
  }
  const now = new Date();
  return due.toDateString() === now.toDateString() || due.getTime() < now.getTime();
}
