import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

// P2: 主题早期初始化（避免首屏闪烁）
(function initThemeEarly() {
  try {
    const theme = (localStorage.getItem("theme") as "dark" | "light" | "system") || "dark";
    const effective =
      theme === "system"
        ? window.matchMedia("(prefers-color-scheme: dark)").matches
          ? "dark"
          : "light"
        : theme;
    const root = document.documentElement;
    root.classList.add(effective);
  } catch {
    document.documentElement.classList.add("dark");
  }
})();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
