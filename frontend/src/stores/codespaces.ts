// Codespaces domain store (ADR 0007). Keyed by project for the list view, plus a
// per-id cache for the detail page. Interactive file/exec/git calls are not
// cached here — components call the api client directly for those request/response
// round-trips; the store owns the codespace records themselves.
import { create } from "zustand";
import { api, Codespace } from "../api";

interface CodespacesState {
  byProject: Record<string, Codespace[]>;
  byId: Record<string, Codespace>;
  loadForProject: (projectId: string) => Promise<void>;
  load: (id: string) => Promise<Codespace>;
  create: (projectId: string, name?: string) => Promise<Codespace>;
  rename: (id: string, name: string) => Promise<Codespace>;
  remove: (id: string, projectId: string) => Promise<void>;
}

export const useCodespaces = create<CodespacesState>((set) => ({
  byProject: {},
  byId: {},
  loadForProject: async (projectId) => {
    const list = await api.codespaces(projectId);
    set((s) => ({ byProject: { ...s.byProject, [projectId]: list } }));
  },
  load: async (id) => {
    const cs = await api.codespace(id);
    set((s) => ({ byId: { ...s.byId, [id]: cs } }));
    return cs;
  },
  create: async (projectId, name) => {
    const cs = await api.createCodespace(projectId, name?.trim() || undefined);
    set((s) => ({
      byProject: {
        ...s.byProject,
        [projectId]: [cs, ...(s.byProject[projectId] || [])],
      },
      byId: { ...s.byId, [cs.id]: cs },
    }));
    return cs;
  },
  rename: async (id, name) => {
    const cs = await api.renameCodespace(id, name.trim());
    set((s) => {
      const list = s.byProject[cs.project_id];
      return {
        byId: { ...s.byId, [id]: cs },
        byProject: list
          ? { ...s.byProject, [cs.project_id]: list.map((c) => (c.id === id ? cs : c)) }
          : s.byProject,
      };
    });
    return cs;
  },
  remove: async (id, projectId) => {
    await api.deleteCodespace(id);
    set((s) => ({
      byProject: {
        ...s.byProject,
        [projectId]: (s.byProject[projectId] || []).filter((c) => c.id !== id),
      },
    }));
  },
}));
