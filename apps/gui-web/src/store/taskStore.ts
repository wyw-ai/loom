import { create } from "zustand";
import type { Task } from "@/ipc/types";

export interface TaskStore {
  // State
  tasks: Task[];

  // Setters
  setTasks: (tasks: Task[] | ((prev: Task[]) => Task[])) => void;
}

export const useTaskStore = create<TaskStore>((set) => ({
  tasks: [],

  setTasks: (tasks) => set((state) => ({ tasks: typeof tasks === 'function' ? tasks(state.tasks) : tasks })),
}));
