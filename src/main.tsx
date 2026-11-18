import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { Toaster } from "sonner";
import "@fontsource-variable/geist";
import "@fontsource-variable/outfit";
import "./index.css";
import App from "./App.tsx";
import { routeOutsideLinks } from "./lib/outsideLinks";
import { queryClient } from "./lib/queryClient";

routeOutsideLinks();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <App />
        <Toaster richColors position="bottom-left" closeButton />
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
);
