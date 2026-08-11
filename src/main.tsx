import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { AppTooltipProvider } from "./components/AppTooltip";
import { MeetingEndPromptWindow } from "./components/MeetingEndPromptWindow";
import "./design-tokens.css";
import "./styles.css";

const rootView = new URLSearchParams(window.location.search).get("view");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <AppTooltipProvider>
      {rootView === "meeting-end-prompt" ? <MeetingEndPromptWindow /> : <App />}
    </AppTooltipProvider>
  </React.StrictMode>,
);
