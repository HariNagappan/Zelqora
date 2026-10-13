import { create } from "zustand";

import { api } from "../services/tauri";
import type { TaskCounts, TaskInput, TaskPatch, TaskSort, TaskView, Todo } from "../types";

interface TaskStore {
  tasks: Todo[];
  counts: TaskCounts;
  view: TaskView;
  query: string;
  sort: TaskSort;
  loading: boolean;
  saving: boolean;
  /** Set when a query fails, so the screen can show an error state with retry. */
  error: string | null;
  /** Mirrors the backend so "today" is computed in one place (SQL/chrono). */
  today: { pendingToday: number; overdue: number; completedToday: number; openTotal: number };

  load: (options?: { silent?: boolean }) => Promise<void>;
  setView: (view: TaskView) => void;
  setQuery: (query: string) => void;
  setSort: (sort: TaskSort) => void;
  create: (input: TaskInput) => Promise<Todo | null>;
  update: (id: number, patch: TaskPatch) => Promise<Todo | null>;
  toggle: (id: number, completed: boolean) => Promise<void>;
  remove: (id: number) => Promise<Todo | null>;
  clearCompleted: () => Promise<number>;
}

const emptyCounts: TaskCounts = { all: 0, pending: 0, completed: 0, overdue: 0 };
const emptyToday = { pendingToday: 0, overdue: 0, completedToday: 0, openTotal: 0 };

export const useTaskStore = create<TaskStore>((set, get) => ({
  tasks: [],
  counts: emptyCounts,
  view: "all",
  query: "",
  sort: "due",
  loading: true,
  saving: false,
  error: null,
  today: emptyToday,

  load: async (options) => {
    const { view, query, sort } = get();
    if (!options?.silent) {
      set({ loading: true });
    }
    try {
      const [tasks, counts, today] = await Promise.all([
        api.listTasks({ view, query, sort }),
        api.getTaskCounts(),
        api.getTodayOverview(),
      ]);
      set({ tasks, counts, today, loading: false, error: null });
    } catch (error) {
      set({ loading: false, error: String(error) });
    }
  },

  setView: (view) => {
    set({ view });
    void get().load();
  },
  setQuery: (query) => {
    set({ query });
    void get().load({ silent: true });
  },
  setSort: (sort) => {
    set({ sort });
    void get().load({ silent: true });
  },

  create: async (input) => {
    set({ saving: true });
    try {
      const created = await api.createTask(input);
      await get().load({ silent: true });
      return created;
    } catch (error) {
      set({ error: String(error) });
      return null;
    } finally {
      set({ saving: false });
    }
  },

  update: async (id, patch) => {
    set({ saving: true });
    try {
      const updated = await api.updateTask(id, patch);
      await get().load({ silent: true });
      return updated;
    } catch (error) {
      set({ error: String(error) });
      return null;
    } finally {
      set({ saving: false });
    }
  },

  toggle: async (id, completed) => {
    // Optimistic: the row updates instantly, then the authoritative list and
    // counts replace it. A failure rolls the change back.
    const previous = get().tasks;
    set({
      tasks: previous.map((task) => (task.id === id ? { ...task, completed } : task)),
    });
    try {
      await api.setTodoCompleted(id, completed);
      await get().load({ silent: true });
    } catch (error) {
      set({ tasks: previous, error: String(error) });
    }
  },

  remove: async (id) => {
    const removed = get().tasks.find((task) => task.id === id) ?? null;
    try {
      await api.deleteTodo(id);
      await get().load({ silent: true });
      return removed;
    } catch (error) {
      set({ error: String(error) });
      return null;
    }
  },

  clearCompleted: async () => {
    try {
      const removed = await api.clearCompletedTodos();
      await get().load({ silent: true });
      return removed;
    } catch (error) {
      set({ error: String(error) });
      return 0;
    }
  },
}));
