// Admin domain store (ADR 0007): cross-tenant stats, builds, and user roles.
import { create } from "zustand";
import { AdminBuild, AdminSprite, AdminStats, AdminUser, api } from "../api";

interface AdminState {
  stats: AdminStats | null;
  builds: AdminBuild[];
  sprites: AdminSprite[];
  users: AdminUser[];
  statusFilter: string;
  loading: boolean;
  spritesLoading: boolean;
  setStatusFilter: (status: string) => void;
  loadStats: () => Promise<void>;
  loadBuilds: () => Promise<void>;
  loadSprites: () => Promise<void>;
  loadUsers: () => Promise<void>;
  setRole: (id: string, role: "user" | "admin") => Promise<void>;
  rebuild: (buildId: string) => Promise<void>;
  deleteSprite: (name: string) => Promise<void>;
  setSpritePublic: (name: string) => Promise<void>;
}

export const useAdmin = create<AdminState>((set, get) => ({
  stats: null,
  builds: [],
  sprites: [],
  users: [],
  statusFilter: "",
  loading: false,
  spritesLoading: false,
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
  loadSprites: async () => {
    set({ spritesLoading: true });
    try {
      set({ sprites: await api.adminSprites() });
    } finally {
      set({ spritesLoading: false });
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
  deleteSprite: async (name) => {
    await api.adminDeleteSprite(name);
    // Drop it locally; sprites.dev is the source of truth and the next
    // loadSprites() reconciles if the delete didn't fully take.
    set((s) => ({ sprites: s.sprites.filter((sp) => sp.name !== name) }));
  },
  setSpritePublic: async (name) => {
    await api.adminSetSpritePublic(name);
  },
}));
