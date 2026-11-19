import { QueryClient } from "@tanstack/react-query";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: { networkMode: "always", refetchOnWindowFocus: true, staleTime: 10_000 },
    mutations: { networkMode: "always" },
  },
});
