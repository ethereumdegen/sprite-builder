// Docuspaces domain store (ADR 0007). Keyed by project for the list view, plus a
// per-id cache for the detail page. The file read/write/list calls are not cached
// here — components call the api client directly for those round-trips; the store
// owns the docuspace records themselves.
import { create } from "zustand";
import { api, Docuspace } from "../api";

interface DocuspacesState {
  byProject: Record<string, Docuspace[]>;
  byId: Record<string, Docuspace>;
  loadForProject: (projectId: string) => Promise<void>;
  load: (id: string) => Promise<Docuspace>;
  create: (projectId: string, name?: string) => Promise<Docuspace>;
  rename: (id: string, name: string) => Promise<Docuspace>;
  remove: (id: string, projectId: string) => Promise<void>;
}

export const useDocuspaces = create<DocuspacesState>((set) => ({
  byProject: {},
  byId: {},
  loadForProject: async (projectId) => {
    const list = await api.docuspaces(projectId);
    set((s) => ({ byProject: { ...s.byProject, [projectId]: list } }));
  },
  load: async (id) => {
    const ds = await api.docuspace(id);
    set((s) => ({ byId: { ...s.byId, [id]: ds } }));
    return ds;
  },
  create: async (projectId, name) => {
    const ds = await api.createDocuspace(projectId, name?.trim() || undefined);
    set((s) => ({
      byProject: {
        ...s.byProject,
        [projectId]: [ds, ...(s.byProject[projectId] || [])],
      },
      byId: { ...s.byId, [ds.id]: ds },
    }));
    return ds;
  },
  rename: async (id, name) => {
    const ds = await api.renameDocuspace(id, name.trim());
    set((s) => {
      const list = s.byProject[ds.project_id];
      return {
        byId: { ...s.byId, [id]: ds },
        byProject: list
          ? { ...s.byProject, [ds.project_id]: list.map((d) => (d.id === id ? ds : d)) }
          : s.byProject,
      };
    });
    return ds;
  },
  remove: async (id, projectId) => {
    await api.deleteDocuspace(id);
    set((s) => ({
      byProject: {
        ...s.byProject,
        [projectId]: (s.byProject[projectId] || []).filter((d) => d.id !== id),
      },
    }));
  },
}));
