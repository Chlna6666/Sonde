import React from "react";
import ReactDOM from "react-dom/client";
import { initI18n } from "./i18n";
import "./styles.css";
import { App } from "./App";
import { initializeTheme } from "./lib/theme";

initializeTheme();
await initI18n();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode><App /></React.StrictMode>
);
