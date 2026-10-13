// Task editor modal: create and edit a task with validation and saving state.

import { useEffect, useRef, useState } from "react";

import { Button, Field, Select, Segmented, TextInput } from "./ui";
import { api } from "../services/tauri";
import { useTaskStore } from "../stores/useTasks";
import { toastError, toastSuccess } from "../stores/useToasts";
import { fromDateTimeLocal, toDateTimeLocal } from "../lib/format";
import type { Priority, Todo } from "../types";

const CATEGORIES = ["College", "Career", "Personal", "Errands"];

export default function TaskEditor({
  task,
  onClose,
}: {
  /** null → create a new task. */
  task: Todo | null;
  onClose: () => void;
}) {
  const create = useTaskStore((store) => store.create);
  const update = useTaskStore((store) => store.update);
  const [title, setTitle] = useState(task?.title ?? "");
  const [description, setDescription] = useState(task?.description ?? "");
  const [category, setCategory] = useState(task?.category ?? CATEGORIES[0]);
  const [priority, setPriority] = useState<Priority>(task?.priority ?? "medium");
  const [due, setDue] = useState(toDateTimeLocal(task?.due_at ?? null));
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const titleRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    titleRef.current?.focus();
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [onClose]);

  const submit = async () => {
    const value = title.trim();
    if (!value) {
      setError("Give the task a name so Nova can find it later.");
      titleRef.current?.focus();
      return;
    }
    setSaving(true);
    setError(null);
    try {
      if (task) {
        const updated = await update(task.id, {
          title: value,
          description,
          category,
          priority,
          due: fromDateTimeLocal(due) ?? "",
          clearDue: !due,
        });
        if (!updated) {
          throw new Error("the task could not be saved");
        }
        toastSuccess("Task updated", `“${updated.title}” was saved.`);
      } else {
        const created = await create({
          title: value,
          description,
          category,
          priority,
          due: fromDateTimeLocal(due),
          source: "manual",
        });
        if (!created) {
          throw new Error("the task could not be saved");
        }
        toastSuccess("Task added", `“${created.title}” · ${created.category || "no category"}`);
      }
      onClose();
    } catch (err) {
      setError(String(err));
      toastError("Couldn't save the task", String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      className="dlgHost"
      onClick={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div className="dlg" role="dialog" aria-modal="true" aria-label={task ? "Edit task" : "Add task"}>
        <h3>{task ? "Edit task" : "Add task"}</h3>

        <div style={{ marginTop: 14 }}>
          <Field label="Title *" error={error} htmlFor="task-title">
            <input
              id="task-title"
              className="inp"
              ref={titleRef}
              value={title}
              maxLength={120}
              aria-invalid={Boolean(error)}
              onChange={(event) => {
                setTitle(event.target.value);
                if (error) {
                  setError(null);
                }
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  void submit();
                }
              }}
            />
          </Field>
        </div>

        <div className="row" style={{ gap: 12, marginTop: 12, alignItems: "flex-end" }}>
          <div className="fld grow">
            <label className="lbl" htmlFor="task-due">
              Due date
            </label>
            <TextInput
              id="task-due"
              type="datetime-local"
              value={due}
              onChange={(event) => setDue(event.target.value)}
            />
          </div>
          <div className="fld">
            <span className="lbl">Priority</span>
            <Segmented
              label="Priority"
              value={priority}
              onChange={(next) => setPriority(next)}
              options={[
                { id: "low", label: "Low" },
                { id: "medium", label: "Med" },
                { id: "high", label: "High" },
              ]}
            />
          </div>
        </div>

        <div style={{ marginTop: 12 }}>
          <Field label="Category" htmlFor="task-category">
            <Select id="task-category" value={category} onChange={(event) => setCategory(event.target.value)}>
              {CATEGORIES.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </Select>
          </Field>
        </div>

        <div style={{ marginTop: 12 }}>
          <Field label="Description (optional)" htmlFor="task-description">
            <textarea
              id="task-description"
              className="inp"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
            />
          </Field>
        </div>

        {task?.due_at ? (
          <button
            className="more"
            style={{ marginTop: 10, textDecoration: "underline" }}
            onClick={() => setDue("")}
          >
            Remove the due date
          </button>
        ) : null}

        <div className="dacts">
          <Button variant="sec" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="pri" disabled={saving} onClick={() => void submit()}>
            {saving ? (
              <>
                <span className="spn" />
                Saving…
              </>
            ) : task ? (
              "Save changes"
            ) : (
              "Add task"
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
