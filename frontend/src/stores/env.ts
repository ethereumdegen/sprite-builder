// Per-project environment variables domain store (ADR 0007). Keyed by project.
import { create } from "zustand";
import { api, ProjectEnvVar } from "../api";

interface EnvState {
  byProject: Record<string, ProjectEnvVar[]>;
  load: (projectId: string) => Promise<void>;
  upsert: (projectId: string, key: string, value: string) => Promise<void>;
  remove: (projectId: string, key: string) => Promise<void>;
}

export const useEnvVars = create<EnvState>((set) => ({
  byProject: {},
  load: async (projectId) => {
    const vars = await api.envVars(projectId);
    set((s) => ({ byProject: { ...s.byProject, [projectId]: vars } }));
  },
  upsert: async (projectId, key, value) => {
    await api.setEnvVar(projectId, key, value);
    const vars = await api.envVars(projectId);
    set((s) => ({ byProject: { ...s.byProject, [projectId]: vars } }));
  },
  remove: async (projectId, key) => {
    await api.deleteEnvVar(projectId, key);
    const vars = await api.envVars(projectId);
    set((s) => ({ byProject: { ...s.byProject, [projectId]: vars } }));
  },
}));
