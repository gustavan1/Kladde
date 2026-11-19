import { MutationCache, QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { currentLang, errorDetail } from "./api";

export const queryClient = new QueryClient({
  // A failed save, delete or tick must never look like it worked. Mutations that show their
  // own error (onError, or an inline message marked with meta.errorShownInline) are left alone.
  mutationCache: new MutationCache({
    onError: (error, _variables, _result, mutation) => {
      if (mutation.options.onError || mutation.options.meta?.errorShownInline) return;
      const da = currentLang() !== "en";
      toast.error(errorDetail(error, da ? "Ændringen kunne ikke gemmes. Prøv igen." : "The change could not be saved. Try again."));
    },
  }),
  defaultOptions: {
    queries: { networkMode: "always", refetchOnWindowFocus: true, staleTime: 10_000 },
    mutations: { networkMode: "always" },
  },
});
