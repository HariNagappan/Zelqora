import { Icon } from "./Icon";
import { Checkbox, Chip, IconButton } from "./ui";
import { formatDue, isOverdue, priorityLabel } from "../lib/format";
import type { Todo } from "../types";

/** One task row — the compact, scannable unit from the Tasks design. */
export default function TaskRow({
  task,
  onToggle,
  onEdit,
  onDelete,
  busy = false,
}: {
  task: Todo;
  onToggle: (task: Todo) => void;
  onEdit?: (task: Todo) => void;
  onDelete?: (task: Todo) => void;
  busy?: boolean;
}) {
  const overdue = isOverdue(task.due_at, task.completed);
  const due = formatDue(task.due_at, task.completed);
  const priorityClass = task.priority === "high" ? "chip pr-h" : task.priority === "low" ? "chip" : "chip pr-m";

  return (
    <div className={`tk${task.completed ? " done" : ""}`} data-id={task.id}>
      <Checkbox
        checked={task.completed}
        label={task.completed ? `Reopen ${task.title}` : `Complete ${task.title}`}
        onChange={() => (busy ? undefined : onToggle(task))}
      />
      <span className="tt" title={task.title}>
        {task.title}
      </span>
      {task.source === "voice" ? (
        <Chip icon="mic" title="Created by voice">
          <span className="sr-only" />
        </Chip>
      ) : null}
      {due ? (
        <span className={overdue ? "chip od" : "chip"} title={overdue ? "Past due" : "Due date"}>
          <Icon name={task.completed ? "check" : "cal"} />
          {due}
        </span>
      ) : null}
      <span className={priorityClass} title="Priority">
        <Icon name="flag" />
        {priorityLabel(task.priority)}
      </span>
      {task.category ? <Chip title="Category">{task.category}</Chip> : null}
      <span className="acts">
        {onEdit ? (
          <IconButton
            icon="pen"
            label={`Edit ${task.title}`}
            onClick={() => onEdit(task)}
            disabled={busy}
          />
        ) : null}
        {onDelete ? (
          <IconButton
            icon="trash"
            label={`Delete ${task.title}`}
            onClick={() => onDelete(task)}
            disabled={busy}
          />
        ) : null}
      </span>
    </div>
  );
}
