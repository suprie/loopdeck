import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { RootErrorBoundary } from "./components/shared/RootErrorBoundary";

// Fonts bundled for offline-first use (replaces Google Fonts <link> in index.html).
import "@fontsource/inter/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "@fontsource/inter/700.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <RootErrorBoundary>
      <App />
    </RootErrorBoundary>
  </React.StrictMode>,
);
