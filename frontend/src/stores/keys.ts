// API-keys domain store (ADR 0007).
import { create } from "zustand";
import { api, ApiKey } from "../api";

interface KeysState {
  keys: ApiKey[];
  load: () => Promise<void>;
  create: (name: string) => Promise<{ key: ApiKey; secret: string }>;
  remove: (id: string) => Promise<void>;
}

export const useKeys = create<KeysState>((set, get) => ({
  keys: [],
  load: async () => {
    set({ keys: await api.keys() });
  },
  create: async (name) => {
    const res = await api.createKey(name);
    await get().load();
    return res;
  },
  remove: async (id) => {
    await api.deleteKey(id);
    await get().load();
  },
}));
