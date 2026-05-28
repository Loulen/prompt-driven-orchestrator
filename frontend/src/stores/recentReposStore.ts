import { create } from "zustand";
import { fetchRecentRepos } from "../api";

interface RecentReposState {
  recentRepos: string[];
  refresh: () => Promise<void>;
}

export const useRecentReposStore = create<RecentReposState>((set) => ({
  recentRepos: [],
  refresh: async () => {
    try {
      const repos = await fetchRecentRepos();
      set({ recentRepos: repos });
    } catch {
      // keep stale data on failure
    }
  },
}));
