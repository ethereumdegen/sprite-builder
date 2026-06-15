// Builds domain store (ADR 0007). Keyed by project for the list view, plus a
// per-build cache for the live detail/diagnostics panel.
import { create } from "zustand";
import { api, Build, RuntimeLogs } from "../api";

interface BuildsState {
  byProject: Record<string, Build[]>;
  byId: Record<string, Build>;
  runtimeById: Record<string, RuntimeLogs>;
  loadForProject: (projectId: string) => Promise<void>;
  loadBuild: (id: string) => Promise<void>;
  loadRuntime: (id: string) => Promise<void>;
  create: (projectId: string, commit?: string) => Promise<Build>;
}

export const useBuilds = create<BuildsState>((set) => ({
  byProject: {},
  byId: {},
  runtimeById: {},
  loadForProject: async (projectId) => {
    const builds = await api.builds(projectId);
    set((s) => ({ byProject: { ...s.byProject, [projectId]: builds } }));
  },
  loadBuild: async (id) => {
    const build = await api.build(id);
    set((s) => ({ byId: { ...s.byId, [id]: build } }));
  },
  loadRuntime: async (id) => {
    const runtime = await api.runtimeLogs(id);
    set((s) => ({ runtimeById: { ...s.runtimeById, [id]: runtime } }));
  },
  create: async (projectId, commit) => {
    const build = await api.createBuild(projectId, commit?.trim() || undefined);
    set((s) => ({
      byProject: {
        ...s.byProject,
        [projectId]: [build, ...(s.byProject[projectId] || [])],
      },
      byId: { ...s.byId, [build.id]: build },
    }));
    return build;
  },
}));
