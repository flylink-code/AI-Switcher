import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import "@/styles/tokens/index.css";
import "@/i18n"; // side-effect: initialize i18next before the app renders
import "@/styles.css";
import { queryClient } from "@/lib/queryClient";
import { applyPlatformAgentVisibility } from "@/lib/agentVisibility";
import { getAppPlatform } from "@/services/system";

// Agent option lists are built when their modules load, so the host OS has to be
// known before App (and everything it imports) is evaluated.
async function bootstrap() {
  applyPlatformAgentVisibility(await getAppPlatform().catch(() => ""));
  const [{ default: App }, { initializeProviderHealthEvents }, { initializeProxyStatusEvents }] =
    await Promise.all([
      import("@/App"),
      import("@/stores/providersStore"),
      import("@/lib/proxyStatusEvents"),
    ]);
  initializeProviderHealthEvents();
  initializeProxyStatusEvents();

  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>
    </React.StrictMode>,
  );
}

void bootstrap();
