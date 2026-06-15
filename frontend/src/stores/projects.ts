// Projects domain store (ADR 0007).
import { create } from "zustand";
import { api, Project } from "../api";

interface CreateProjectInput {
  name: string;
  repo_full_name: string;
  repo_id?: number;
  default_branch?: string;
  dockerfile_path?: string;
  container_port?: number;
}

interface ProjectsState {
  projects: Project[];
  loading: boolean;
  load: () => Promise<void>;
  create: (body: CreateProjectInput) => Promise<Project>;
}

export const useProjects = create<ProjectsState>((set, get) => ({
  projects: [],
  loading: true,
  load: async () => {
    set({ loading: true });
    try {
      set({ projects: await api.projects() });
    } finally {
      set({ loading: false });
    }
  },
  create: async (body) => {
    const project = await api.createProject(body);
    await get().load();
    return project;
  },
}));
