import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { MutationCache, QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import App from "./App";
import { toast } from "./ui";
import "./styles.css";

const queryClient = new QueryClient({
  // Every failed mutation is surfaced once here; mutations that show the error inline set meta.quiet.
  mutationCache: new MutationCache({ onError: (e, _v, _c, m) => !m.meta?.quiet && toast(e.message) }),
  defaultOptions: { queries: { staleTime: 30_000, refetchOnWindowFocus: true, retry: 1 } },
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
);
