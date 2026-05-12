import { create } from "zustand";

import type { Task, TaskAssignment } from "@/ipc/types";

interface TasksState {
  tasks: Task[];
  assignmentsById: Record<string, TaskAssignment>;
  replaceTasks: (tasks: Task[]) => void;
  upsertTask: (task: Task) => void;
  upsertAssignment: (assignment: TaskAssignment) => void;
  clear: () => void;
}

function sortTasks(tasks: Task[]) {
  return tasks
    .slice()
    .sort((a, b) =>
      a.channelId === b.channelId
        ? a.number - b.number
        : a.channelId.localeCompare(b.channelId),
    );
}

export const useTasks = create<TasksState>((set) => ({
  tasks: [],
  assignmentsById: {},
  replaceTasks: (tasks) => set({ tasks: sortTasks(tasks) }),
  upsertTask: (task) =>
    set((s) => {
      const idx = s.tasks.findIndex((row) => row.id === task.id);
      const tasks =
        idx >= 0
          ? s.tasks.map((row) => (row.id === task.id ? task : row))
          : [...s.tasks, task];
      return { tasks: sortTasks(tasks) };
    }),
  upsertAssignment: (assignment) =>
    set((s) => ({
      assignmentsById: { ...s.assignmentsById, [assignment.id]: assignment },
    })),
  clear: () => set({ tasks: [], assignmentsById: {} }),
}));
