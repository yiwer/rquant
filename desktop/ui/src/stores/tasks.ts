import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import type { TaskInfoDto } from "@bindings/TaskInfoDto";
import { api } from "../api/ipc";

interface TasksState {
  tasks: Record<string, TaskInfoDto>;
  startedAt: Record<string, number>;
  inited: boolean;
  ingest: (info: TaskInfoDto) => void;
  init: () => void;
}

export const useTasks = create<TasksState>((set, get) => ({
  tasks: {},
  startedAt: {},
  inited: false,
  ingest: (info) =>
    set((s) => ({
      tasks: { ...s.tasks, [info.id]: info },
      startedAt: s.startedAt[info.id] ? s.startedAt : { ...s.startedAt, [info.id]: Date.now() },
    })),
  init: () => {
    if (get().inited) return;
    set({ inited: true });
    void api.taskList().then((list) => list.forEach((t) => get().ingest(t))).catch(() => {});
    void listen<TaskInfoDto>("task://progress", (e) => get().ingest(e.payload));
  },
}));

/** 订阅全局 store,任务到终态时一次性回调并退订(已终态则立即回调)。 */
export function trackTask(
  id: string,
  handlers: { done?: (info: TaskInfoDto) => void; failed?: (info: TaskInfoDto) => void; cancelled?: (info: TaskInfoDto) => void },
): void {
  const fire = (info: TaskInfoDto): boolean => {
    if (info.status === "done") { handlers.done?.(info); return true; }
    if (info.status === "failed") { handlers.failed?.(info); return true; }
    if (info.status === "cancelled") { handlers.cancelled?.(info); return true; }
    return false;
  };
  const cur = useTasks.getState().tasks[id];
  if (cur && fire(cur)) return;
  const unsub = useTasks.subscribe((s) => {
    const info = s.tasks[id];
    if (info && fire(info)) unsub();
  });
}

export const useTaskInfo = (id: string | null): TaskInfoDto | undefined =>
  useTasks((s) => (id ? s.tasks[id] : undefined));
export const useTaskStartedAt = (id: string | null): number | undefined =>
  useTasks((s) => (id ? s.startedAt[id] : undefined));
