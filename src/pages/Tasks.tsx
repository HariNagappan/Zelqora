// Task management (design Screen 3): filters, search, sort, create/edit/delete.

import { useEffect, useRef, useState } from "react";

import TaskEditor from "../components/TaskEditor";
import TaskRow from "../components/TaskRow";
import { Icon } from "../components/Icon";
import {
  Banner,
  Button,
  Card,
  EmptyState,
  SearchField,
  Select,
  SkeletonTasks,
} from "../components/ui";
import { confirmAction } from "../components/Dialog";
import { useNovaStore } from "../stores/useNova";
import { useTaskStore } from "../stores/useTasks";
import { toast, toastInfo, toastSuccess } from "../stores/useToasts";
import { OFFLINE_COPY, useOnline } from "../lib/online";
import type { TaskSort, TaskView, Todo } from "../types";

const VIEWS: { id: TaskView; label: string }[] = [
  { id: "all", label: "All" },
  { id: "pending", label: "Pending" },
  { id: "completed", label: "Completed" },
  { id: "overdue", label: "Overdue" },
];

export default function Tasks() {
  const tasks = useTaskStore();
  const activityBump = useNovaStore((store) => store.activityBump);
  const online = useOnline();
  const [editorOpen, setEditorOpen] = useState(false);
  const [editing, setEditing] = useState<Todo | null>(null);

  // Keep the list in step with voice commands handled in the background.
  const bumpRef = useRef(activityBump);
  useEffect(() => {
    if (bumpRef.current !== activityBump) {
      bumpRef.current = activityBump;
      void tasks.load({ silent: true });
    }
  }, [activityBump, tasks]);

  const openCreate = () => {
    setEditing(null);
    setEditorOpen(true);
  };

  const remove = (task: Todo) => {
    confirmAction({
      title: "Delete this task?",
      icon: "trash",
      destructive: true,
      confirmLabel: "Delete task",
      body: <p>“{task.title}” will be removed from your list. This can’t be undone.</p>,
      onConfirm: async () => {
        const removed = await tasks.remove(task.id);
        if (removed) {
          toast("ok", "Task deleted", `“${removed.title}” was removed.`, {
            label: "Restore",
            run: async () => {
              await tasks.create({
                title: removed.title,
                description: removed.description,
                category: removed.category,
                priority: removed.priority,
                due: removed.due_at,
                source: removed.source,
              });
              toastInfo("Restored", `“${removed.title}” is back on your list.`);
            },
          });
        }
      },
    });
  };

  const clearCompleted = () => {
    confirmAction({
      title: "Clear completed tasks?",
      icon: "trash",
      destructive: true,
      confirmLabel: "Clear tasks",
      body: <p>Every completed task will be removed from your list. This can’t be undone.</p>,
      onConfirm: async () => {
        const removed = await tasks.clearCompleted();
        toastSuccess("Completed tasks cleared", `${removed} task${removed === 1 ? "" : "s"} removed.`);
      },
    });
  };

  const countFor = (view: TaskView) =>
    view === "all"
      ? tasks.counts.all
      : view === "pending"
        ? tasks.counts.pending
        : view === "completed"
          ? tasks.counts.completed
          : tasks.counts.overdue;

  return (
    <>
      <div className="ph">
        <div>
          <h1>Tasks</h1>
          <p className="mut">
            {tasks.counts.pending} pending · {tasks.counts.completed} completed
            {tasks.counts.overdue > 0 ? ` · ${tasks.counts.overdue} overdue` : ""}
          </p>
        </div>
        <Button variant="pri" icon="plus" onClick={openCreate}>
          Add task
        </Button>
      </div>

      {!online ? <Banner kind="wn">{OFFLINE_COPY}</Banner> : null}

      {tasks.error ? (
        <Banner kind="er">
          {tasks.error}
          <button
            className="more"
            style={{ marginLeft: 8, textDecoration: "underline" }}
            onClick={() => void tasks.load()}
          >
            Retry
          </button>
        </Banner>
      ) : null}

      <div className="tools">
        <SearchField
          value={tasks.query}
          onChange={(value) => tasks.setQuery(value)}
          placeholder="Search tasks"
          label="Search tasks"
        />
        <div className="fchips" role="group" aria-label="Filter tasks">
          {VIEWS.map((view) => (
            <button
              key={view.id}
              className={`fchip${tasks.view === view.id ? " on" : ""}`}
              aria-pressed={tasks.view === view.id}
              onClick={() => tasks.setView(view.id)}
            >
              {view.label} <span className="n">{countFor(view.id)}</span>
            </button>
          ))}
        </div>
        <span className="grow" />
        {tasks.counts.completed > 0 ? (
          <Button variant="gst" small icon="trash" onClick={clearCompleted}>
            Clear completed
          </Button>
        ) : null}
        <Select
          aria-label="Sort tasks"
          style={{ width: 158 }}
          value={tasks.sort}
          onChange={(event) => tasks.setSort(event.target.value as TaskSort)}
        >
          <option value="due">Sort: Due date</option>
          <option value="priority">Sort: Priority</option>
          <option value="created">Sort: Created</option>
        </Select>
      </div>

      <Card>
        {tasks.loading ? (
          <div className="cbd" style={{ paddingTop: 16 }}>
            <SkeletonTasks rows={5} />
          </div>
        ) : tasks.tasks.length === 0 ? (
          <div className="cbd" style={{ paddingTop: 16 }}>
            {tasks.query ? (
              <EmptyState
                icon="search"
                title={`No matches for “${tasks.query}”`}
                detail="Try a shorter word, or clear the search."
                action={
                  <Button variant="sec" onClick={() => tasks.setQuery("")}>
                    Clear search
                  </Button>
                }
              />
            ) : tasks.view === "overdue" ? (
              <EmptyState
                icon="check"
                title="No overdue tasks"
                detail="You’re all caught up — nothing past due."
                action={
                  <Button variant="sec" icon="plus" onClick={openCreate}>
                    Add a task
                  </Button>
                }
              />
            ) : tasks.view === "completed" ? (
              <EmptyState
                icon="check"
                title="Nothing completed yet"
                detail="Finished tasks land here."
              />
            ) : (
              <EmptyState
                icon="tasks"
                title="Your list is empty"
                detail="Add the first task here, or say “Hey Nova, add finish my assignment to my to-dos.”"
                action={
                  <Button variant="pri" icon="plus" onClick={openCreate}>
                    Add your first task
                  </Button>
                }
              />
            )}
          </div>
        ) : (
          <div className="cbd" style={{ paddingTop: 16 }}>
            <div className="tlst">
              {tasks.tasks.map((task) => (
                <TaskRow
                  key={task.id}
                  task={task}
                  busy={tasks.saving}
                  onToggle={(next) => void tasks.toggle(next.id, !next.completed)}
                  onEdit={(next) => {
                    setEditing(next);
                    setEditorOpen(true);
                  }}
                  onDelete={remove}
                />
              ))}
            </div>
            <p className="dim" style={{ fontSize: 12, marginTop: 12 }}>
              Showing {tasks.tasks.length} of {tasks.counts.all} · voice-created tasks carry a{" "}
              <Icon name="mic" size={11} /> mark
            </p>
          </div>
        )}
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
