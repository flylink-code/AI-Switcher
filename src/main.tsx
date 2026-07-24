import React from "react";
import ReactDOM from "react-dom/client";
import "@/i18n"; // side-effect: initialize i18next before the app renders
import App from "@/App";
import "@/styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
