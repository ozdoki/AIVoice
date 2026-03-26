import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import { FloatingBar } from "./components/FloatingBar";
import "./index.css";

const label = getCurrentWindow().label;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {label === "floating-bar" ? <FloatingBar /> : <App />}
  </React.StrictMode>
);
