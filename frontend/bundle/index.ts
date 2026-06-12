import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

type DeviceHubTerminalHandle = {
  term: Terminal;
  fitAddon: FitAddon;
  resizeObserver: ResizeObserver;
  themeObserver: MutationObserver | null;
  scrollDisposable: { dispose(): void } | null;
  onViewportChange: ((atBottom: boolean) => void) | null;
};

declare global {
  interface Window {
    deviceHubCreateTerminal: (container: HTMLElement) => DeviceHubTerminalHandle;
    deviceHubCreateTerminalWithViewportHandler: (
      container: HTMLElement,
      onViewportChange?: (atBottom: boolean) => void,
    ) => DeviceHubTerminalHandle;
    deviceHubAppendTerminal: (handle: DeviceHubTerminalHandle, text: string) => void;
    deviceHubResetTerminal: (handle: DeviceHubTerminalHandle) => void;
    deviceHubScrollToBottom: (handle: DeviceHubTerminalHandle) => void;
    deviceHubFitTerminal: (handle: DeviceHubTerminalHandle) => void;
    deviceHubDisposeTerminal: (handle: DeviceHubTerminalHandle) => void;
  }
}

function isNearBottom(term: Terminal): boolean {
  const buffer = term.buffer.active;
  return buffer.viewportY >= buffer.baseY - 2;
}

function emitViewportState(handle: DeviceHubTerminalHandle): void {
  handle.onViewportChange?.(isNearBottom(handle.term));
}

const terminalThemes = {
  dracula: {
    background: "#282a36",
    foreground: "#f8f8f2",
    cursor: "#f8f8f2",
    selectionBackground: "rgba(189, 147, 249, 0.34)",
    black: "#21222c",
    red: "#ff5555",
    green: "#50fa7b",
    yellow: "#f1fa8c",
    blue: "#bd93f9",
    magenta: "#ff79c6",
    cyan: "#8be9fd",
    white: "#f8f8f2",
    brightBlack: "#6272a4",
    brightRed: "#ff6e6e",
    brightGreen: "#69ff94",
    brightYellow: "#ffffa5",
    brightBlue: "#d6acff",
    brightMagenta: "#ff92df",
    brightCyan: "#a4ffff",
    brightWhite: "#ffffff",
  },
  light: {
    background: "#f8fafc",
    foreground: "#0f172a",
    cursor: "#0f172a",
    selectionBackground: "rgba(0, 109, 119, 0.22)",
    black: "#0f172a",
    red: "#dc2626",
    green: "#047857",
    yellow: "#b45309",
    blue: "#2563eb",
    magenta: "#9333ea",
    cyan: "#0e7490",
    white: "#f8fafc",
    brightBlack: "#64748b",
    brightRed: "#ef4444",
    brightGreen: "#059669",
    brightYellow: "#d97706",
    brightBlue: "#3b82f6",
    brightMagenta: "#a855f7",
    brightCyan: "#06b6d4",
    brightWhite: "#ffffff",
  },
};

function currentThemeName(): keyof typeof terminalThemes {
  return document.documentElement.dataset.theme === "light" ? "light" : "dracula";
}

function currentTerminalTheme() {
  return terminalThemes[currentThemeName()];
}

function createTerminal(
  container: HTMLElement,
  onViewportChange?: (atBottom: boolean) => void,
): DeviceHubTerminalHandle {
  const term = new Terminal({
    allowTransparency: true,
    convertEol: false,
    cursorBlink: false,
    disableStdin: true,
    drawBoldTextInBrightColors: false,
    fontFamily:
      '"Iosevka Term", "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace',
    fontSize: 13,
    lineHeight: 1.35,
    macOptionIsMeta: true,
    scrollback: 10000,
    theme: currentTerminalTheme(),
  });
  const fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  term.open(container);
  const scrollDisposable = term.onScroll(() => {
    onViewportChange?.(isNearBottom(term));
  });

  const resizeObserver = new ResizeObserver(() => {
    fitAddon.fit();
  });
  resizeObserver.observe(container);

  const themeObserver = new MutationObserver(() => {
    term.options.theme = currentTerminalTheme();
  });
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme"],
  });

  fitAddon.fit();
  onViewportChange?.(true);

  return {
    term,
    fitAddon,
    resizeObserver,
    themeObserver,
    scrollDisposable,
    onViewportChange: onViewportChange ?? null,
  };
}

function appendTerminal(handle: DeviceHubTerminalHandle, text: string): void {
  if (!text) {
    return;
  }

  const stickToBottom = isNearBottom(handle.term);
  handle.term.write(text, () => {
    if (stickToBottom) {
      handle.term.scrollToBottom();
    }
    emitViewportState(handle);
  });
}

function resetTerminal(handle: DeviceHubTerminalHandle): void {
  handle.term.reset();
  handle.term.clear();
  emitViewportState(handle);
}

function scrollToBottom(handle: DeviceHubTerminalHandle): void {
  handle.term.scrollToBottom();
  emitViewportState(handle);
}

function fitTerminal(handle: DeviceHubTerminalHandle): void {
  handle.fitAddon.fit();
}

function disposeTerminal(handle: DeviceHubTerminalHandle): void {
  handle.resizeObserver.disconnect();
  handle.themeObserver?.disconnect();
  handle.scrollDisposable?.dispose();
  handle.term.dispose();
}

window.deviceHubCreateTerminal = (container: HTMLElement) => createTerminal(container);
window.deviceHubCreateTerminalWithViewportHandler = createTerminal;
window.deviceHubAppendTerminal = appendTerminal;
window.deviceHubResetTerminal = resetTerminal;
window.deviceHubScrollToBottom = scrollToBottom;
window.deviceHubFitTerminal = fitTerminal;
window.deviceHubDisposeTerminal = disposeTerminal;

export {};
