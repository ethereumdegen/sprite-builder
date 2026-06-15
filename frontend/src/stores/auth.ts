// Auth domain store (ADR 0007). Holds the current user and exposes a
// capability check used to gate UI (ADR 0016 — gate on capabilities, not roles).
import { create } from "zustand";
import { api, Capability, User } from "../api";

interface AuthState {
  user: User | null;
  loading: boolean;
  loadMe: () => Promise<void>;
  logout: () => Promise<void>;
  setUser: (u: User | null) => void;
  can: (cap: Capability) => boolean;
}

export const useAuth = create<AuthState>((set, get) => ({
  user: null,
  loading: true,
  loadMe: async () => {
    try {
      const u = await api.me();
      set({ user: u });
    } catch {
      set({ user: null });
    } finally {
      set({ loading: false });
    }
  },
  logout: async () => {
    await api.logout();
    set({ user: null });
  },
  setUser: (u) => set({ user: u }),
  can: (cap) => !!get().user?.capabilities.includes(cap),
}));
