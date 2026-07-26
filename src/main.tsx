import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import "antd/dist/antd.css";
import "@/i18n"; // side-effect: initialize i18next before the app renders
import App from "@/App";
import "@/styles.css";
import { queryClient } from "@/lib/queryClient";
import { initializeProviderHealthEvents } from "@/stores/providersStore";
import { initializeProxyStatusEvents } from "@/lib/proxyStatusEvents";

initializeProviderHealthEvents();
initializeProxyStatusEvents();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
);
