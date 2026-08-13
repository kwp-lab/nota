import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { AppTooltipProvider } from "./components/AppTooltip";
import { CapturePromptWindow } from "./components/CapturePromptWindow";
import "./design-tokens.css";
import "./styles.css";

const rootView = new URLSearchParams(window.location.search).get("view");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <AppTooltipProvider>
      {rootView === "capture-prompt" ? (
        <CapturePromptWindow />
      ) : (
        <App />
      )}
    </AppTooltipProvider>
  </React.StrictMode>,
);
