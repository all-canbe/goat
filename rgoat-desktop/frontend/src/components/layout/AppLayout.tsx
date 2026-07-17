import { useState, useCallback, useEffect } from "react";
import Sidebar from "./Sidebar";
import ChatArea from "../chat/ChatArea";
import StatusBar from "../status/StatusBar";
import TitleBar from "./TitleBar";
import RightPanel from "./RightPanel";
import ApprovalDialog from "../dialogs/ApprovalDialog";
import PlanPreviewDialog from "../dialogs/PlanPreviewDialog";
import SettingsDialog from "../dialogs/SettingsDialog";
import CommandPalette from "../command/CommandPalette";
import Toaster from "../feedback/Toaster";
import { useAgentEvents } from "../../hooks/useAgentEvents";
import { useGlobalShortcuts } from "../../hooks/useGlobalShortcuts";
import { useConfigStore } from "../../stores/configStore";
import { useSessionStore } from "../../stores/sessionStore";
import { useChatStore } from "../../stores/chatStore";
import { useChangesStore } from "../../stores/changesStore";
import { useThemeStore } from "../../stores/themeStore";

// P1: Sidebar 折叠状态 localStorage key
const SIDEBAR_COLLAPSED_KEY = "sidebar_collapsed";

export default function AppLayout() {
  const { isStreaming } = useAgentEvents();
  const { loadProviders } = useConfigStore();
  const [currentMode, setCurrentMode] = useState("Agent");
  const [isCommandPaletteOpen, setCommandPaletteOpen] = useState(false);
  // P2: 设置页 Modal 状态
  const [isSettingsOpen, setSettingsOpen] = useState(false);
  // P1: 从 localStorage 初始化折叠状态
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    try {
      return localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "true";
    } catch {
