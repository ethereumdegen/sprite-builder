// Admin domain store (ADR 0007): cross-tenant stats, builds, and user roles.
import { create } from "zustand";
import { AdminBuild, AdminStats, AdminUser, api } from "../api";

interface AdminState {
  stats: AdminStats | null;
  builds: AdminBuild[];
  users: AdminUser[];
  statusFilter: string;
  loading: boolean;
  setStatusFilter: (status: string) => void;
  loadStats: () => Promise<void>;
  loadBuilds: () => Promise<void>;
  loadUsers: () => Promise<void>;
  setRole: (id: string, role: "user" | "admin") => Promise<void>;
  rebuild: (buildId: string) => Promise<void>;
}

export const useAdmin = create<AdminState>((set, get) => ({
  stats: null,
  builds: [],
  users: [],
  statusFilter: "",
  loading: false,
  setStatusFilter: (status) => set({ statusFilter: status }),
  loadStats: async () => {
    set({ stats: await api.adminStats() });
  },
  loadBuilds: async () => {
    set({ loading: true });
    try {
      set({ builds: await api.adminBuilds(get().statusFilter || undefined) });
    } finally {
      set({ loading: false });
    }
  },
  loadUsers: async () => {
    set({ users: await api.adminUsers() });
  },
  setRole: async (id, role) => {
    const updated = await api.adminSetRole(id, role);
    set((s) => ({ users: s.users.map((u) => (u.id === id ? updated : u)) }));
  },
  rebuild: async (buildId) => {
    const created = await api.adminRebuild(buildId);
    // Prepend the new queued build so it shows immediately; the 5s refresh
    // will then keep its status live.
    set((s) => ({ builds: [created, ...s.builds] }));
  },
}));
