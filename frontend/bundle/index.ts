import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

type DeviceHubTerminalHandle = {
  term: Terminal;
  fitAddon: FitAddon;
  resizeObserver: ResizeObserver;
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
    theme: {
      background: "#111827",
      foreground: "#e5e7eb",
      cursor: "#f8fafc",
      selectionBackground: "rgba(148, 163, 184, 0.28)",
      black: "#111827",
      red: "#f87171",
      green: "#34d399",
      yellow: "#fbbf24",
      blue: "#60a5fa",
      magenta: "#c084fc",
      cyan: "#22d3ee",
      white: "#e5e7eb",
      brightBlack: "#64748b",
      brightRed: "#fca5a5",
      brightGreen: "#6ee7b7",
      brightYellow: "#fcd34d",
      brightBlue: "#93c5fd",
      brightMagenta: "#d8b4fe",
      brightCyan: "#67e8f9",
      brightWhite: "#f8fafc",
    },
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

  fitAddon.fit();
  onViewportChange?.(true);

  return {
    term,
    fitAddon,
    resizeObserver,
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
