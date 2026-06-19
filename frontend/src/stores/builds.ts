// Builds domain store (ADR 0007). Keyed by project for the list view, plus a
// per-build cache for the live detail/diagnostics panel.
import { create } from "zustand";
import { api, Build, DeploymentStatus, RuntimeLogs, UrlVisibility } from "../api";

interface BuildsState {
  byProject: Record<string, Build[]>;
  byId: Record<string, Build>;
  runtimeById: Record<string, RuntimeLogs>;
  visibilityById: Record<string, UrlVisibility>;
  deploymentById: Record<string, DeploymentStatus>;
  loadForProject: (projectId: string) => Promise<void>;
  loadBuild: (id: string) => Promise<void>;
  loadRuntime: (id: string) => Promise<void>;
  loadVisibility: (id: string) => Promise<void>;
  setVisibility: (id: string, isPublic: boolean) => Promise<void>;
  loadDeployment: (id: string) => Promise<void>;
  create: (projectId: string, commit?: string) => Promise<Build>;
  remove: (id: string, projectId: string) => Promise<void>;
}

export const useBuilds = create<BuildsState>((set) => ({
  byProject: {},
  byId: {},
  runtimeById: {},
  visibilityById: {},
  deploymentById: {},
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
  loadVisibility: async (id) => {
    const vis = await api.urlVisibility(id);
    set((s) => ({ visibilityById: { ...s.visibilityById, [id]: vis } }));
  },
  setVisibility: async (id, isPublic) => {
    const vis = await api.setUrlVisibility(id, isPublic);
    set((s) => ({ visibilityById: { ...s.visibilityById, [id]: vis } }));
  },
  loadDeployment: async (id) => {
    const dep = await api.deployment(id);
    set((s) => ({ deploymentById: { ...s.deploymentById, [id]: dep } }));
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
  remove: async (id, projectId) => {
    await api.deleteBuild(id);
    set((s) => {
      const byId = { ...s.byId };
      delete byId[id];
      return {
        byId,
        byProject: {
          ...s.byProject,
          [projectId]: (s.byProject[projectId] || []).filter((b) => b.id !== id),
        },
      };
    });
  },
}));
