// The island: DOM shell, sizing animation, Mochi placement, mouse handling.
// Mirrors IslandRootView.swift + IslandWindowController.swift.

import { Tracked, Spring, clamp } from "../core/anim";
import { Bridge, IS_TAURI, onDragDrop, onEvent, type AntigravityLiveTask, type BatteryStatus, type BluetoothStatus, type DownloadStatus, type PrivacyStatus } from "../core/bridge";
import {
  COMPACT_CHARGING_R,
  COMPACT_BT_R,
  EXPANDED_CORNER, EXPANDED_W, NOTCH_W, PANEL_H, PANEL_W,
  ROUNDED_CORNER, VIEW_LAYOUTS, botGlowColor, botGlowOpacity, botPosition, chatPromptHeight,
  islandSize,
  QUESTION_PICKER_H,
  MINIMIZED_PILL_W, MINIMIZED_PILL_H, MINIMIZED_PILL_R,
  type BotEmoteName, type BotStateName, type IslandMode, type IslandViewName,
} from "../core/layout";
import { Sound } from "../core/sound";
import { State } from "../core/state";
import { SPOTIFY_ID, Spotify, isSpotifyTab, islandDances } from "../core/spotify";
import { BotEngine, hexToRGB } from "../mochi/engine";
import { Greeting } from "../mochi/greeting";
import { createMiniBot, pruneMiniBots, syncMiniBotStates, tickMiniBots } from "../mochi/minibots";
import { SeasonCache, parseOutfit } from "../mochi/wardrobe";
import { UploadCanvas } from "../upload/canvas";
import { USC, UploadSeq } from "../upload/sequence";
import { closePlanCard, openPlanColor, planCardOpen } from "../views/usage";
import { buildHeader, buildViews, type ViewActions, type ViewHost } from "../views/views";
import { h, svg } from "../views/dom";
import { ICONS } from "../views/icons";
import { t } from "../i18n/i18n";
import { IslandStateMachine } from "./fsm";
import { refreshHookPills } from "./integrations";
import { DesktopLink } from "./desktop";
import type { ViewCommand } from "./shortcuts";
import { DRAG_THRESHOLD } from "../mochi/desktop-logic";

const BOT_OVERHANG = 40;
const CLAUDE_DESKTOP_ID = "agent_claude-desktop";
/** Extra canvas on each side of Mochi, for the witch hat's brim and the Santa hat's tip. */
const BOT_SIDE = 24;
/** Same margin as the Rust hit test (src-tauri/src/island.rs). */
const HIT_MARGIN = 14;

/** The three views the drop sequence owns; leaving them stops the engine. */
const UPLOAD_VIEWS: ReadonlySet<IslandViewName> = new Set(["upload", "uploading", "choose"]);

/** Seconds between the drop and the moment the progress bar starts filling. */
const PRE_PROGRESS = USC.T_PROG_START - USC.T_DROP;

const modeOrder = (m: IslandMode) => (m === "hidden" ? 0 : m === "compact" ? 1 : 2);

export class Island {
  readonly fsm = new IslandStateMachine();
  /** Mochi on the desktop: his life cycle and the drag out of the island. */
  readonly desktop: DesktopLink;

  private root: HTMLElement;
  private islandEl!: HTMLElement;
  private clipEl!: HTMLElement;
  private contentEl!: HTMLElement;
  private viewsEl!: HTMLElement;
  private botCanvas!: HTMLCanvasElement;
  private botGlow!: HTMLElement;
  private greetingCanvas!: HTMLCanvasElement;
  private miniGrid!: HTMLElement;
  private countdown!: HTMLElement;
  private wakeStrip!: HTMLElement;
  private chargingBanner!: HTMLElement;
  private chargingRing!: HTMLElement;
  private chargingTitle!: HTMLElement;
  private chargingSub!: HTMLElement;
  private chargingPercent!: HTMLElement;
  private batteryPopupTimer: number | null = null;
  private batteryBannerMode: "charging" | "low" | null = null;

  private bluetoothBanner!: HTMLElement;
  private bluetoothRing!: HTMLElement;
  private bluetoothProgressCircle!: SVGCircleElement;
  private bluetoothTitle!: HTMLElement;
  private bluetoothSub!: HTMLElement;
  private bluetoothRight!: HTMLElement;
  private bluetoothPopupTimer: number | null = null;
  private bluetoothDisconnectedMode = false;
  private lastDisconnectedName = "";

  private downloadBanner!: HTMLElement;
  private downloadProgressCircle!: SVGCircleElement;
  private downloadTitle!: HTMLElement;
  private downloadSub!: HTMLElement;
  private downloadPercent!: HTMLElement;
  private downloadPopupTimer: number | null = null;

  private privacyContainer!: HTMLElement;
  private privacyCameraDot!: HTMLElement;
  private privacyMicDot!: HTMLElement;

  private audioVisualizer!: HTMLElement;
  private compactLyrics!: HTMLElement;
  private compactLyricsText!: HTMLElement;
  private hudToast!: HTMLElement;
  private hudToastIcon!: HTMLElement;
  private hudToastVal!: HTMLElement;
  private hudToastTimer: number | null = null;

  private header!: ViewHost;
  private views!: Map<IslandViewName, ViewHost>;
  private uploadCanvas!: UploadCanvas;

  private width = new Tracked(NOTCH_W);
  private height = new Tracked(0);
  private radius = new Tracked(ROUNDED_CORNER);
  private botCx = new Spring(46);
  private botCy = new Spring(16);
  private botSize = new Spring(10);

  private engine = new BotEngine();
  private greeting = new Greeting();
  private greetingShown = false;
  private seasons = new SeasonCache();

  private running = false;
  private lastFrame = 0;
  private dirty = true;
  private canvasPx = 0;

  // Rust starts the window at full size so the launch greeting has room.
  private collapsed = false;
  private collapseTimer: number | null = null;
  private wasInIsland = false;
  /** Last shape handed to Rust for the click-through test. */
  private pushedRect = { x: -1, y: -1, w: -1, h: -1 };

  // Bot hover → love (IslandWindowController.botHoverIn)
  private botHovering = false;
  private botHoverTimer: number | null = null;
  private lastLoveTime = 0;
  private botHoverStart = { x: 0, y: 0 };

  private confusedRecovery: number | null = null;
  private prevViewBeforeConfused: IslandViewName = "overview";
  private lastSyncedView: IslandViewName | null = null;

  /** The launch greeting ended, or the island came out of hidden — two of the
   *  moments the Monday recap may open (see src/recap/recap.ts). */
  onGreetingDone: (() => void) | null = null;
  onWake: (() => void) | null = null;

  /** Where a press on Mochi started: moving past DRAG_THRESHOLD drags him out. */
  private botPress: { x: number; y: number } | null = null;

  /** The next reveal from hidden makes no peek (music starting, as on macOS). */
  private silentReveal = false;

  /** Drop sequence bookkeeping: last tick played, and whether the ✓ has fired. */
  private uploadTens = 0;
  private uploadDone = false;

  constructor(root: HTMLElement) {
    this.root = root;
    this.desktop = new DesktopLink({
      reveal: () => this.reveal(),
      wardrobeFromDesktop: () => this.wardrobeFromDesktop(),
      dizzyFromDesktop: () => this.handleDizzy(),
    });
    this.build();
    this.wireFsm();
    this.wireInput();
    this.syncIslandBehavior();
    void onEvent<boolean>("browser-tabs-blocked", (blocked) => {
      this.handleBrowserTabsBlocked(blocked);
    });
    void onEvent<BatteryStatus>("battery-status", (status) => {
      this.handleBatteryStatus(status);
    });
    void Bridge.batteryStatus().then((status) => {
      if (status) {
        State.batteryStatus = status;
        this.animateGeometry(false);
        State.notify();
      }
    });
    void onEvent<BluetoothStatus>("bluetooth-status", (status) => {
      this.handleBluetoothStatus(status);
    });
    void Bridge.bluetoothStatus().then((status) => {
      if (status) {
        State.bluetoothStatus = status;
        this.animateGeometry(false);
        State.notify();
      }
    });
    void onEvent<PrivacyStatus>("privacy-status", (status) => {
      this.handlePrivacyStatus(status);
    });
    void Bridge.privacyStatus().then((status) => {
      if (status) {
        State.privacyStatus = status;
        this.dirty = true;
      }
    });
    void onEvent<DownloadStatus>("download-status", (status) => {
      this.handleDownloadStatus(status);
    });
    void onEvent<string>("download-complete", (filename) => {
      this.handleDownloadComplete(filename);
    });
    void onEvent<AntigravityLiveTask | null>("antigravity-live-task", (task) => {
      this.handleAntigravityTask(task);
    });
    void Bridge.antigravityLiveTask().then((task) => {
      if (task) {
        this.handleAntigravityTask(task);
      }
    });
    this.engine.onDizzy = () => this.handleDizzy();
    this.greeting.onComplete = () => {
      this.fsm.greetComplete();
      this.onGreetingDone?.();
    };
    State.subscribe(() => {
      this.dirty = true;
      this.syncIslandBehavior();
      this.animateGeometry(false);
      this.ensureRunning();
    });
  }

  private handleBrowserTabsBlocked(blocked: boolean) {
    if ((State.settings.islandBehavior || "always_standby") !== "auto_minimize_tabs") return;
    if (blocked) {
      if (this.fsm.state !== "hidden" && !this.wasInIsland && !this.fsm.pinned) {
        this.fsm.forceHidden();
      }
    } else {
      if (this.fsm.state === "hidden" && !this.wasInIsland) {
        this.fsm.reveal();
      }
    }
  }

  private handleBatteryStatus(status: BatteryStatus) {
    const wasCharging = State.batteryStatus.charging;
    const wasLevel = State.batteryStatus.level;
    State.batteryStatus = status;

    // 1. Charger connected event (Image 1 / iOS Dynamic Island popup)
    if (!wasCharging && status.charging) {
      Sound.play("pop");
      if (this.fsm.state === "hidden") {
        this.fsm.reveal();
      }
      this.batteryBannerMode = "charging";
      this.fsm.pinned = true;
      if (this.batteryPopupTimer != null) {
        window.clearTimeout(this.batteryPopupTimer);
      }
      this.batteryPopupTimer = window.setTimeout(() => {
        this.batteryPopupTimer = null;
        this.batteryBannerMode = null;
        this.fsm.pinned = false;
        this.animateGeometry(true);
        State.notify();
      }, 5000);
    } else if (!status.charging && status.hasBattery && status.level <= 20 && (wasLevel > 20 || wasCharging)) {
      // 2. Low battery event (<= 20% and not plugged in)
      Sound.play("pop");
      if (this.fsm.state === "hidden") {
        this.fsm.reveal();
      }
      this.batteryBannerMode = "low";
      this.fsm.pinned = true;
      if (this.batteryPopupTimer != null) {
        window.clearTimeout(this.batteryPopupTimer);
      }
      this.batteryPopupTimer = window.setTimeout(() => {
        this.batteryPopupTimer = null;
        this.batteryBannerMode = null;
        this.fsm.pinned = false;
        this.animateGeometry(true);
        State.notify();
      }, 6000);
    }

    this.animateGeometry(false);
    State.notify();
  }

  private handlePrivacyStatus(status: PrivacyStatus) {
    State.privacyStatus = status;
    this.dirty = true;
  }

  private handleDownloadStatus(status: DownloadStatus) {
    const wasActive = State.downloadStatus.active;
    State.downloadStatus = status;
    if (!wasActive && status.active) {
      if (this.fsm.state === "hidden") {
        this.fsm.reveal();
      }
    }
    this.animateGeometry(false);
    State.notify();
  }

  private handleDownloadComplete(filename: string) {
    Sound.play("pop");
    State.downloadCompleteFile = filename;
    State.downloadStatus = {
      active: true,
      filename,
      speedBytesPerSec: 0,
      downloadedBytes: 0,
      totalBytes: null,
      percent: 100,
    };
    if (this.fsm.state === "hidden") {
      this.fsm.reveal();
    }
    this.fsm.pinned = true;
    if (this.downloadPopupTimer != null) {
      window.clearTimeout(this.downloadPopupTimer);
    }
    this.downloadPopupTimer = window.setTimeout(() => {
      this.downloadPopupTimer = null;
      this.fsm.pinned = false;
      State.downloadCompleteFile = null;
      State.downloadStatus.active = false;
      this.animateGeometry(true);
      State.notify();
    }, 4500);
    this.animateGeometry(false);
    State.notify();
  }

  private handleAntigravityTask(task: AntigravityLiveTask | null) {
    if (!task) {
      const existing = State.tasks.find((x) => x.id === "agent_antigravity");
      if (existing && (existing.state === "working" || existing.state === "thinking")) {
        existing.state = "finished";
        this.dirty = true;
        State.notify();
      }
      return;
    }
    let t = State.tasks.find((x) => x.id === "agent_antigravity");
    if (!t) {
      State.upsertExternalAgent("agent_antigravity", "Antigravity", "#E879F9");
      t = State.tasks.find((x) => x.id === "agent_antigravity");
    }
    if (t) {
      const wasWorking = t.state === "working" || t.state === "thinking";
      t.state = (task.state as BotStateName) || "working";
      t.statusText = task.statusText;
      t.steps = task.steps;
      t.stepIndex = Math.max(0, task.steps.length - 1);
      if (task.finalLine) t.finalLine = task.finalLine;
      if (task.workspacePath) t.sessionCwd = task.workspacePath;
      if (task.sessionId) t.sessionId = task.sessionId;
      if (!wasWorking && (t.state === "working" || t.state === "thinking")) {
        if (this.fsm.state === "hidden") {
          this.fsm.reveal();
        }
      }
      this.dirty = true;
      State.notify();
    }
  }

  private handleBluetoothStatus(status: BluetoothStatus) {
    const wasConnected = State.bluetoothStatus.connected;
    const oldName = State.bluetoothStatus.name;
    State.bluetoothStatus = status;
    if (!wasConnected && status.connected) {
      this.bluetoothDisconnectedMode = false;
      Sound.play("pop");
      if (this.fsm.state === "hidden") {
        this.fsm.reveal();
      }
      // Hold open for 5 seconds as a popup notification even if tabs are blocked
      this.fsm.pinned = true;
      if (this.bluetoothPopupTimer != null) {
        window.clearTimeout(this.bluetoothPopupTimer);
      }
      this.bluetoothPopupTimer = window.setTimeout(() => {
        this.fsm.pinned = false;
        this.bluetoothPopupTimer = null;
        if (this.fsm.islandBehavior === "auto_minimize_tabs") {
          void Bridge.browserTabsBlocked().then((blocked) => {
            if (blocked && this.fsm.state !== "hidden" && !this.wasInIsland) {
              this.fsm.forceHidden();
            }
          });
        }
      }, 5000);
    } else if (wasConnected && !status.connected) {
      this.bluetoothDisconnectedMode = true;
      this.lastDisconnectedName = status.name || oldName || "Bluetooth Device";
      Sound.play("pop");
      if (this.fsm.state === "hidden") {
        this.fsm.reveal();
      }
      this.fsm.pinned = true;
      if (this.bluetoothPopupTimer != null) {
        window.clearTimeout(this.bluetoothPopupTimer);
      }
      this.bluetoothPopupTimer = window.setTimeout(() => {
        this.bluetoothDisconnectedMode = false;
        this.fsm.pinned = false;
        this.bluetoothPopupTimer = null;
        this.animateGeometry(true);
        State.notify();
        if (this.fsm.islandBehavior === "auto_minimize_tabs") {
          void Bridge.browserTabsBlocked().then((blocked) => {
            if (blocked && this.fsm.state !== "hidden" && !this.wasInIsland) {
              this.fsm.forceHidden();
            }
          });
        }
      }, 4000);
    }
    this.animateGeometry(false);
    State.notify();
  }

  private syncIslandBehavior() {
    const behavior = State.settings.islandBehavior || "always_standby";
    this.fsm.islandBehavior = behavior;
    if (behavior === "always_standby") {
      if (this.fsm.state === "hidden") {
        this.fsm.reveal();
      }
    } else if (behavior === "auto_minimize_tabs") {
      void Bridge.browserTabsBlocked().then((blocked) => {
        if (blocked != null) this.handleBrowserTabsBlocked(blocked);
      });
    }
  }

  /** The request has its answer: the card goes and the session carries on. */
  private closeApproval() {
    State.endApproval();
    this.fsm.pinned = false;
    this.setView(State.defaultView());
  }

  /**
   * Folds a card that is waiting for an answer down to the compact island,
   * without answering it (Mac #290). Nothing is decided: the request keeps
   * waiting, the island stays on screen, and opening it shows the card again.
   */
  foldApproval() {
    if (!State.pendingApproval || State.mode !== "expanded") return;
    State.isPinned = true;
    this.fsm.pinned = true;
    this.fsm.forcePetit();
  }

  // ── DOM ─────────────────────────────────────────────────────────────────────

  private build() {
    const actions: ViewActions = {
      setView: (v) => this.setView(v),
      cancelDrop: () => this.discardDrop(),
      collapse: () => this.collapse(),
      foldApproval: () => this.foldApproval(),
      setFocus: (id) => {
        State.setFocus(id);
        Sound.play("blip");
        // A pill with a waiting request opens on its card: going back to it
        // after looking at another pill brings the card up again.
        const req = State.pendingApproval;
        if (req?.pillId === id) this.setView(req.questions ? "question" : "approval");
      },
      openTerminal: () => {
        const task = State.focusTask;
        // Sessions from the Claude desktop app live there, not in a terminal.
        if (task?.id === CLAUDE_DESKTOP_ID) void Bridge.openClaudeDesktop();
        else void Bridge.openSession(task?.sessionId ?? null, task?.sessionCwd ?? null);
      },
      // The ↗ button — same targets as openAgentTarget() on macOS.
      openTarget: () => {
        const task = State.focusTask;
        if (!task) return;
        const urls: Record<string, string> = {
          integration_resend: "https://resend.com/emails",
          integration_vercel: "https://vercel.com/dashboard",
          integration_github: "https://github.com/pulls",
          integration_stripe: "https://dashboard.stripe.com/payments",
          integration_notion: "https://notion.so",
          integration_calcom: "https://app.cal.com/bookings",
        };
        if (task.id === CLAUDE_DESKTOP_ID) void Bridge.openClaudeDesktop();
        else if (task.id === "integration_claude" || task.sessionId) {
          void Bridge.openSession(task.sessionId ?? null, task.sessionCwd ?? null);
        } else if (task.id === "integration_n8n") void Bridge.openN8n();
        else if (task.id === SPOTIFY_ID) void Bridge.spotifyOpen();
        else if (urls[task.id]) void Bridge.openUrl(urls[task.id]);
      },
      openUrl: (url) => {
        if (url) void Bridge.openUrl(url);
      },
      decide: (d) => {
        const req = State.pendingApproval;
        void Bridge.log(`decide ${d} req=${req?.requestId ?? "none"}`);
        if (!req) return;
        Sound.play(d === "deny" ? "blip" : "approve");
        void Bridge.approvalDecision(req.requestId, d);
        this.closeApproval();
      },
      answer: (answers) => {
        const req = State.pendingApproval;
        if (!req) return;
        Sound.play("approve");
        void Bridge.approvalAnswer(req.requestId, answers);
        this.closeApproval();
      },
      answerInTerminal: () => {
        const req = State.pendingApproval;
        if (!req) return;
        Sound.play("blip");
        void Bridge.approvalDecline(req.requestId);
        this.closeApproval();
      },
      toggleSound: () => {
        State.settings.soundEnabled = !State.settings.soundEnabled;
        Sound.setEnabled(State.settings.soundEnabled);
        void Bridge.saveSettings(State.settings);
        State.notify();
      },
      setVolume: (v) => {
        State.settings.soundVolume = v;
        Sound.setVolume(v);
        void Bridge.saveSettings(State.settings);
        State.notify();
      },
      setAutoClose: (s) => {
        State.settings.autoCloseInterval = s;
        this.fsm.homeToPetitDelay = s;
        void Bridge.saveSettings(State.settings);
        State.notify();
      },
      openSettingsWindow: () => void Bridge.openSettingsWindow(),
      blip: () => Sound.play("blip"),
      chooseOutfit: (selection) => {
        if (parseOutfit(State.settings.mochiOutfit) === selection) return;
        State.settings.mochiOutfit = selection;
        void Bridge.saveSettings(State.settings);
        Sound.play("pop");
        this.engine.triggerEmote("proud");
        State.notify();
      },
      previewOutfit: (outfit) => {
        State.wardrobePreview = outfit;
        State.notify();
      },
    };

    this.wakeStrip = h("div", { id: "wake-strip" });
    this.botGlow = h("div", { id: "bot-glow" });
    this.botCanvas = h("canvas", { id: "bot-canvas" });
    this.greetingCanvas = h("canvas", { id: "greeting-canvas" });
    this.miniGrid = h("div", { id: "mini-grid" });
    this.countdown = h("div", { id: "countdown" });

    this.header = buildHeader(actions);
    this.views = buildViews(actions, () => this.animateGeometry(false));
    this.viewsEl = h("div", { id: "views" });
    for (const v of this.views.values()) this.viewsEl.append(v.el);
    this.contentEl = h("div", { id: "content" }, this.header.el, this.viewsEl);

    // The drop sequence draws the card, the bar and its own Mochi. It sits under
    // the header, which stays visible on top of it exactly as on macOS.
    this.uploadCanvas = new UploadCanvas({
      ask: () => {
        State.promptContext = State.droppedFile
          ? { kind: "file", name: State.droppedFile.name, path: State.droppedFile.path }
          : null;
        this.setView("prompt");
      },
      cancel: () => this.discardDrop(),
    });

    this.clipEl = h(
      "div",
      { id: "island-clip" },
      this.greetingCanvas,
      this.uploadCanvas.el,
      this.contentEl,
    );
    this.chargingRing = h("div", { class: "charging-ring" }, svg(ICONS.bolt, 15, { fill: "#fff" }));
    this.chargingTitle = h("div", { class: "charging-title" }, t("Charging"));
    this.chargingSub = h("div", { class: "charging-sub" }, t("Charger connected"));
    const chargingInfo = h("div", { class: "charging-info" }, this.chargingTitle, this.chargingSub);
    const chargingLeft = h("div", { class: "charging-left" }, this.chargingRing, chargingInfo);
    this.chargingPercent = h("div", { class: "charging-percent" }, "100%");
    this.chargingBanner = h("div", { id: "charging-banner" }, chargingLeft, this.chargingPercent);
    this.chargingBanner.addEventListener("click", () => this.fsm.click());

    // Bluetooth notch banner (Image 3)
    const btSvgCircle = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    btSvgCircle.setAttribute("cx", "16");
    btSvgCircle.setAttribute("cy", "16");
    btSvgCircle.setAttribute("r", "14");
    btSvgCircle.setAttribute("class", "bt-ring-progress");
    const btSvgBg = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    btSvgBg.setAttribute("cx", "16");
    btSvgBg.setAttribute("cy", "16");
    btSvgBg.setAttribute("r", "14");
    btSvgBg.setAttribute("class", "bt-ring-bg");
    const btSvg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    btSvg.setAttribute("viewBox", "0 0 32 32");
    btSvg.setAttribute("class", "bt-ring-svg");
    btSvg.appendChild(btSvgBg);
    btSvg.appendChild(btSvgCircle);
    this.bluetoothProgressCircle = btSvgCircle;

    const btIconInside = h("div", { class: "bt-center-icon" }, svg(ICONS.bluetooth, 12, { fill: "#fff" }));
    this.bluetoothRing = h("div", { class: "bt-ring-wrap" }, btSvg as unknown as HTMLElement, btIconInside);

    this.bluetoothTitle = h("div", { class: "bt-title" }, "Bluetooth");
    this.bluetoothSub = h("div", { class: "bt-sub" }, t("Connected"));
    const btInfo = h("div", { class: "bt-info" }, this.bluetoothTitle, this.bluetoothSub);
    const btLeft = h("div", { class: "bt-left" }, this.bluetoothRing, btInfo);

    const audioGlow = h("div", { class: "bt-audio-glow" });
    const audioInner = h("div", { class: "bt-audio-circle" }, svg(ICONS.headphones, 14, { fill: "#fff" }));
    this.bluetoothRight = h("div", { class: "bt-right" }, audioGlow, audioInner);

    this.bluetoothBanner = h("div", { id: "bluetooth-banner" }, btLeft, this.bluetoothRight);
    this.bluetoothBanner.addEventListener("click", () => this.fsm.click());

    // Download notch banner
    const dlSvgCircle = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    dlSvgCircle.setAttribute("cx", "16");
    dlSvgCircle.setAttribute("cy", "16");
    dlSvgCircle.setAttribute("r", "14");
    dlSvgCircle.setAttribute("class", "dl-ring-progress");
    const dlSvgBg = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    dlSvgBg.setAttribute("cx", "16");
    dlSvgBg.setAttribute("cy", "16");
    dlSvgBg.setAttribute("r", "14");
    dlSvgBg.setAttribute("class", "dl-ring-bg");
    const dlSvg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    dlSvg.setAttribute("viewBox", "0 0 32 32");
    dlSvg.setAttribute("class", "dl-ring-svg");
    dlSvg.appendChild(dlSvgBg);
    dlSvg.appendChild(dlSvgCircle);
    this.downloadProgressCircle = dlSvgCircle;

    const dlIconInside = h("div", { class: "dl-center-icon" }, svg(ICONS.download, 13, { stroke: 2 }));
    const dlRing = h("div", { class: "dl-ring-wrap" }, dlSvg as unknown as HTMLElement, dlIconInside);

    this.downloadTitle = h("div", { class: "dl-title" }, "Mengunduh...");
    this.downloadSub = h("div", { class: "dl-sub" }, "0 KB/s");
    const dlInfo = h("div", { class: "dl-info" }, this.downloadTitle, this.downloadSub);
    const dlLeft = h("div", { class: "dl-left" }, dlRing, dlInfo);
    this.downloadPercent = h("div", { class: "dl-percent" }, "0%");

    this.downloadBanner = h("div", { id: "download-banner" }, dlLeft, this.downloadPercent);
    this.downloadBanner.addEventListener("click", () => this.fsm.click());

    // Privacy indicator dots (Camera & Mic)
    this.privacyCameraDot = h("div", { class: "privacy-dot camera", title: "Kamera / Webcam aktif", style: "display: none;" });
    this.privacyMicDot = h("div", {
      class: "privacy-dot mic",
      title: "Mikrofon aktif (Klik untuk mute)",
      style: "display: none;",
      onclick: (e: Event) => {
        e.stopPropagation();
        void Bridge.toggleMicMute().then((muted) => {
          State.privacyStatus.micMuted = !!muted;
          this.dirty = true;
          State.notify();
        });
      },
    });
    this.privacyContainer = h("div", { class: "privacy-dots-container" }, this.privacyCameraDot, this.privacyMicDot);

    // Live Audio Visualizer (bouncing equalizer waveform bars)
    this.audioVisualizer = h("div", { class: "notch-audio-visualizer", title: "Audio Visualizer" },
      h("span", { class: "eq-bar eq-1" }),
      h("span", { class: "eq-bar eq-2" }),
      h("span", { class: "eq-bar eq-3" }),
      h("span", { class: "eq-bar eq-4" }),
    );

    // Live compact running lyrics in notch
    this.compactLyricsText = h("span", { class: "notch-lyric-text" });
    this.compactLyrics = h("div", { class: "notch-compact-lyrics" }, this.compactLyricsText);

    // Volume / Brightness HUD Toast on Scroll Gesture
    this.hudToastIcon = h("span", { class: "hud-toast-icon" });
    this.hudToastVal = h("span", { class: "hud-toast-val" });
    this.hudToast = h("div", { class: "notch-hud-toast" }, this.hudToastIcon, this.hudToastVal);

    this.islandEl = h(
      "div",
      { id: "island" },
      this.clipEl,
      this.botGlow,
      this.botCanvas,
      this.miniGrid,
      this.compactLyrics,
      this.audioVisualizer,
      this.chargingBanner,
      this.bluetoothBanner,
      this.downloadBanner,
      this.privacyContainer,
      this.hudToast,
      this.countdown,
    );

    // Mouse wheel scroll gesture: scroll on notch to adjust Volume or Brightness
    this.islandEl.addEventListener("wheel", (e: WheelEvent) => {
      const isUp = e.deltaY < 0;
      const step = isUp ? 4 : -4;
      if (e.shiftKey) {
        e.preventDefault();
        void Bridge.changeBrightness(step).then((b) => {
          if (b != null) this.showHudToast("brightness", b);
        });
        return;
      }

      // Volume scroller only works when specifically on the Spotify tab
      const isSpotify = isSpotifyTab({
        mode: State.mode,
        view: State.view,
        focusId: State.focusId,
        focusTaskId: State.focusTask?.id,
      });

      // Do not intercept or adjust volume on other tabs or inside scrollable lyrics
      const targetEl = e.target as HTMLElement | null;
      if (!isSpotify || targetEl?.closest?.(".np-lyrics-container")) {
        return;
      }

      e.preventDefault();
      const cur = Spotify.state.volume;
      const next = Math.max(0, Math.min(100, cur + step));
      if (Spotify.state.running || State.spotifyPlaying || Spotify.state.track != null) {
        Spotify.state = { ...Spotify.state, volume: next };
        void Bridge.spotifyControl("volume", next);
        this.showHudToast("volume", next);
        State.notify();
      } else {
        void Bridge.changeVolume(step).then((v) => {
          if (v != null) this.showHudToast("volume", v);
        });
      }
    }, { passive: false });

    const dpr = Math.min(2, window.devicePixelRatio || 1);
    this.greetingCanvas.width = Math.round(EXPANDED_W * dpr);
    this.greetingCanvas.height = Math.round(150 * dpr);
    this.greetingCanvas.style.width = `${EXPANDED_W}px`;
    this.greetingCanvas.style.height = "150px";

    this.root.append(this.wakeStrip, this.islandEl);
    this.applyGeometry();
  }

  private showHudToast(kind: "volume" | "brightness", val: number) {
    if (this.hudToastTimer != null) {
      window.clearTimeout(this.hudToastTimer);
      this.hudToastTimer = null;
    }
    const iconName = kind === "volume"
      ? (val <= 0 ? ICONS.volume0 : val < 33 ? ICONS.volume1 : val < 66 ? ICONS.volume2 : ICONS.volume3)
      : ICONS.sun;
    this.hudToastIcon.replaceChildren(svg(iconName, 15, { fill: "#fff" }));
    this.hudToastVal.textContent = `${Math.round(val)}%`;
    this.hudToast.style.opacity = "1";
    this.hudToastTimer = window.setTimeout(() => {
      this.hudToast.style.opacity = "0";
      this.hudToastTimer = null;
    }, 1100);
  }

  // ── FSM ─────────────────────────────────────────────────────────────────────

  private wireFsm() {
    this.fsm.homeToPetitDelay = State.settings.autoCloseInterval;
    this.fsm.openOnHover = State.settings.openOnHover;
    this.fsm.islandBehavior = State.settings.islandBehavior || "always_standby";
    this.fsm.onTransition = (from, to) => {
      // The greeting is over, however it ended: back to his desktop spot.
      if (from === "coucou" && to !== "coucou") this.desktop.launch();
      switch (to) {
        case "hidden":
          this.setMode("hidden");
          break;
        case "petit":
          if (from === "coucou") this.greeting.interrupt();
          else if (from === "hidden" && !this.silentReveal) Sound.play("peek");
          this.setMode("compact");
          if (from === "coucou") State.view = State.defaultView();
          if (!this.wasInIsland) this.fsm.mouseLeft();
          break;
        case "home":
          this.expand(State.defaultView());
          if (!this.wasInIsland) this.fsm.mouseLeft();
          // Hooks may have been installed in a terminal since: the idle cards
          // say so on the next open, without polling while the island is shut.
          void refreshHookPills();
          break;
        case "coucou":
          this.expand("greeting");
          this.greeting.start();
          break;
      }
      State.notify();
      if (from === "hidden") this.onWake?.();
    };
  }

  launch() {
    this.fsm.launch();
  }

  // ── Mode / view ─────────────────────────────────────────────────────────────

  private setMode(mode: IslandMode) {
    const prev = State.mode;
    if (mode === prev) return;
    State.mode = mode;
    if (mode === "expanded") Sound.play("open");
    if (prev === "expanded") {
      Sound.play("close");
      // A folded card is still waiting: it keeps the island pinned.
      if (!State.pendingApproval) State.isPinned = false;
      void Bridge.focusWindow(false);
    }
    if (mode !== "expanded") {
      closePlanCard();
      this.engine.resetMorph();
      // Nothing can be seen of the sequence once the island is shut, and leaving
      // it running would keep the frame loop awake — the island must cost
      // nothing while hidden.
      UploadSeq.deactivate();
    }
    this.updateWindowCollapsed();
    this.animateGeometry(modeOrder(mode) < modeOrder(prev));
    State.notify();
  }

  /** True while the drop sequence owns the island body. */
  private get uploadActive(): boolean {
    return State.mode === "expanded" && UploadSeq.isActive && UPLOAD_VIEWS.has(State.view);
  }

  /** Navigating out of the drop flow ends the sequence, as on macOS. */
  private stopSequenceIfLeaving(view: IslandViewName) {
    if (UploadSeq.isActive && !UPLOAD_VIEWS.has(view)) UploadSeq.deactivate();
  }

  expand(view: IslandViewName) {
    this.stopSequenceIfLeaving(view);
    if (view !== "overview") closePlanCard();
    State.view = view;
    if (State.mode !== "expanded") this.setMode("expanded");
    else this.animateGeometry(false);
    State.lastActivity = performance.now();
    State.notify();
  }

  setView(view: IslandViewName) {
    this.stopSequenceIfLeaving(view);
    if (view !== "overview") closePlanCard();
    if (State.mode !== "expanded") {
      this.fsm.forceHome();
      State.view = view;
      this.animateGeometry(false);
      State.notify();
      return;
    }
    const grew = VIEW_LAYOUTS[view].height >= VIEW_LAYOUTS[State.view].height;
    State.view = view;
    State.lastActivity = performance.now();
    this.animateGeometry(!grew);
    State.notify();
  }

  collapse() {
    // A waiting card is only ever folded, never dropped by a close.
    if (State.pendingApproval) {
      this.foldApproval();
      return;
    }
    State.isPinned = false;
    this.fsm.pinned = false;
    // Drive the state machine rather than the mode: setting the mode behind its
    // back left it thinking the island was still open, and a click on the compact
    // island then did nothing — the island could never be reopened.
    this.fsm.forcePetit();
  }

  /** Alert from the hook server: open on this view. Pinned alerts never auto-close. */
  alert(view: IslandViewName) {
    this.fsm.pinned = State.isPinned;
    this.fsm.forceHome();
    this.expand(view);
  }

  reveal() {
    this.fsm.reveal();
  }

  /** Music started playing: the compact island, without the peek sound (musicReveal). */
  revealSilently() {
    this.silentReveal = true;
    this.fsm.reveal();
    this.silentReveal = false;
  }

  /** Right-click on Mochi: wardrobe open ↔ back to the usual view. */
  toggleWardrobe() {
    if (State.paused || State.mode === "hidden") return;
    // The greeting and the drop sequence draw a Mochi of their own.
    if (State.mode === "expanded" && (State.view === "greeting" || this.uploadActive)) return;
    if (State.mode === "expanded" && State.view === "wardrobe") this.setView(State.defaultView());
    else this.setView("wardrobe");
  }

  /**
   * Right-click on the desktop Mochi (macOS openWardrobeFromDesktop): opens the
   * wardrobe from any state, or goes back if it is already open.
   */
  wardrobeFromDesktop() {
    this.wardrobeAnywhere();
  }

  /**
   * The wardrobe from any state — compact or hidden island included — or back
   * to the usual view if it is already open. The desktop Mochi's right-click
   * and the wardrobe shortcut (`open-wardrobe`) both land here.
   */
  wardrobeAnywhere() {
    if (State.mode === "expanded" && State.view === "wardrobe") {
      this.setView(State.defaultView());
      return;
    }
    if (State.paused) return;
    this.alert("wardrobe");
  }

  /** An alert stopped waiting for an answer: let the island auto-close again. */
  dropPin() {
    this.fsm.pinned = false;
    // The countdown the pin held back starts now, if the mouse is elsewhere.
    if (!this.wasInIsland) this.fsm.mouseLeft();
  }

  // ── Keyboard shortcuts (island/shortcuts.ts) ────────────────────────────────

  emote(name: BotEmoteName) {
    this.engine.triggerEmote(name);
    this.ensureRunning();
  }

  /** Ctrl+P: keep the open island from folding away, or let it fold again. */
  setPinned(on: boolean) {
    State.isPinned = on;
    this.fsm.pinned = on;
    if (on) {
      // The countdown bar reads the state machine's deadline, cleared with it.
      this.fsm.cancelTimers();
    } else if (!this.wasInIsland && this.fsm.state === "home") {
      this.fsm.mouseLeft();
    }
    State.notify();
  }

  /** The island takes the keyboard, so its own shortcuts work (Mac: makeKey).
   *  It gives it back when it closes, or when the chat is left. */
  takeKeyboard() {
    void Bridge.focusWindow(true);
  }

  /** The desktop shortcut needs a desktop Mochi: none on GNOME's Wayland. */
  canLeaveIsland(): boolean {
    return this.desktop.supported;
  }

  /** The desktop shortcut (macOS DesktopMochiController.flyOutOrHome). */
  flyOutOrHome() {
    // The greeting and the drop sequence draw a Mochi of their own: he stays
    // for them, as he does for a drag (canDragOut).
    const busy = State.mode === "expanded" && (State.view === "greeting" || this.uploadActive);
    if (busy && !State.mochiOnDesktop) return;
    this.desktop.flyOutOrHome();
  }

  /** Ctrl+O / Ctrl+E go to the view on screen. */
  viewCommand(command: ViewCommand) {
    this.views.get(State.view)?.command?.(command);
  }

  // ── File drop ───────────────────────────────────────────────────────────────

  private onDragDrop(e: { type: string; paths?: string[] }) {
    if (e.type !== "over") void Bridge.log(`drag ${e.type} ${e.paths?.length ?? 0} file(s)`);
    if (State.paused) return;
    switch (e.type) {
      case "enter":
      case "over": {
        if (State.fileDragOver) return;
        State.fileDragOver = true;
        this.engine.animateMorph(1);
        // enterZone must run before the island expands, so the sequence is
        // already active by the time the view becomes `upload`.
        UploadSeq.enterZone(State.mouseInIsland.x, State.mouseInIsland.y);
        this.alert("upload");
        break;
      }
      case "leave": {
        if (!State.fileDragOver) return;
        State.fileDragOver = false;
        this.engine.animateMorph(0);
        // The island deliberately stays open: the drag session is still alive.
        UploadSeq.exitZone();
        State.notify();
        break;
      }
      case "drop": {
        State.fileDragOver = false;
        const path = e.paths?.[0];
        if (!path) {
          this.engine.animateMorph(0);
          this.setView(State.defaultView());
          return;
        }
        this.swallow(path);
        break;
      }
    }
  }

  /**
   * Mochi eats the file. Nothing here waits on the file system: the copy into
   * the inbox runs in the background and swaps the path in when it lands, so a
   * slow disk can never stall the animation — same as FileDropHandler on macOS.
   */
  private swallow(path: string) {
    const name = path.split(/[\\/]/).pop() || "file";
    State.droppedFile = { name, path };
    State.promptContext = { kind: "file", name, path };
    State.chatHistory = [];
    void Bridge.chatReset();

    UploadSeq.performDrop(State.uploadDuration);
    this.uploadTens = 0;
    this.uploadDone = false;

    this.engine.gulp();
    Sound.play("approve");
    this.engine.triggerEmote("happy");
    this.engine.animateMorph(0);

    State.uploadProgress = 0;
    this.setView("uploading");
    this.ensureRunning();

    void Bridge.ingestFile(path)
      .then((file) => {
        // Cancelled (or replaced by another drop) while the copy was running.
        if (State.droppedFile?.path !== path) return;
        State.droppedFile = { name: file.name, path: file.path };
        State.promptContext = { kind: "file", name: file.name, path: file.path };
        State.notify();
      })
      .catch((err) => {
        if (State.droppedFile?.path !== path) return;
        UploadSeq.deactivate();
        State.noteMessage = String(err).replace(/^Error:\s*/, "");
        this.engine.animateMorph(0);
        this.setView("note");
        Sound.play("error");
        window.setTimeout(() => this.setView(State.defaultView()), 2400);
      });
  }

  /** "Cancel" on the dropped file: forget it, so the chat does not pick it up. */
  private discardDrop() {
    State.droppedFile = null;
    State.promptContext = null;
    this.setView(State.defaultView());
  }

  /**
   * Sounds and view changes hung off the canvas timeline: a `tick` every 10 %,
   * the ✓ chime when the bar completes, then `choose` once Mochi has grown back.
   */
  private stepSequence() {
    const since = UploadSeq.sinceDrop();
    if (since == null) return;
    const dur = State.uploadDuration;
    const p = Math.max(0, Math.min(1, (since - PRE_PROGRESS) / dur));

    const tens = Math.floor(p * 10);
    if (tens > this.uploadTens && tens < 10) {
      this.uploadTens = tens;
      Sound.play("tick");
    }

    if (!this.uploadDone && since >= PRE_PROGRESS + dur) {
      this.uploadDone = true;
      Sound.play("approve");
      this.engine.triggerEmote("happy");
    }
    // The extra second is the grow-back, after which the choose card is up.
    if (since >= PRE_PROGRESS + dur + 1 && State.view === "uploading") {
      this.setView("choose");
    }
  }

  // ── Geometry ────────────────────────────────────────────────────────────────

  private targetSize(): { w: number; h: number; r: number } {
    if (State.mode === "hidden") {
      return { w: MINIMIZED_PILL_W, h: MINIMIZED_PILL_H, r: MINIMIZED_PILL_R };
    }
    const isBluetoothCompact = State.mode === "compact" && (State.bluetoothStatus.connected || this.bluetoothDisconnectedMode);
    const isDownloadCompact = State.mode === "compact" && !isBluetoothCompact && State.downloadStatus.active;
    const isChargingCompact = State.mode === "compact" && !isBluetoothCompact && !isDownloadCompact &&
      ((State.isCharging && (State.settings.chargingStatusInIsland ?? true)) || this.batteryBannerMode != null);
    let { w, h } = islandSize(State.mode, State.view, State.chatHistory.length, isChargingCompact, isBluetoothCompact, isDownloadCompact);
    if (State.mode === "expanded" && State.view === "question" && State.pendingApproval?.questions) {
      h = QUESTION_PICKER_H;
    }
    const isMusicCompact = State.mode === "compact" && !isBluetoothCompact && !isDownloadCompact && !isChargingCompact && State.spotifyPlaying;
    if (isMusicCompact && State.activeLyricLine) {
      w = Math.max(w, 320);
    }
    const r = State.mode === "expanded" ? EXPANDED_CORNER : (isBluetoothCompact || isDownloadCompact) ? COMPACT_BT_R : isChargingCompact ? COMPACT_CHARGING_R : ROUNDED_CORNER;
    return { w, h, r };
  }

  private animateGeometry(shrinking: boolean) {
    const { w, h, r } = this.targetSize();
    if (shrinking) {
      this.width.curveTowards(w);
      this.height.curveTowards(h);
      this.radius.curveTowards(r);
    } else {
      this.width.springTo(w);
      this.height.springTo(h);
      this.radius.springTo(r);
    }
    this.ensureRunning();
  }

  private applyGeometry() {
    const w = this.width.value;
    const hh = this.height.value;
    const r = this.radius.value;
    this.islandEl.style.width = `${w}px`;
    this.islandEl.style.height = `${hh}px`;
    this.islandEl.style.borderRadius = State.mode === "hidden" ? `${r}px` : `0 0 ${r}px ${r}px`;
    this.islandEl.classList.toggle("minimized-pill", State.mode === "hidden");
    this.islandEl.style.transform = `translateX(-50%)`;
    // These follow the island as it resizes, so they belong here rather than in
    // the state-driven DOM sync.
    this.miniGrid.style.left = `${w - 40 - 14.5}px`;
    this.miniGrid.style.top = `${hh / 2 - 14.5}px`;
    this.greetingCanvas.style.left = `${(w - EXPANDED_W) / 2}px`;
    this.uploadCanvas.el.style.left = `${(w - EXPANDED_W) / 2}px`;

    const viewportW = window.innerWidth > 0 ? window.innerWidth : PANEL_W;
    const rect = { x: (viewportW - w) / 2, y: 0, w, h: hh };
    const p = this.pushedRect;
    if (Math.abs(p.x - rect.x) > 0.5 || Math.abs(p.w - rect.w) > 0.5 || Math.abs(p.h - rect.h) > 0.5) {
      this.pushedRect = rect;
      void Bridge.setIslandRect(rect.x, rect.y, rect.w, rect.h);
    }
  }

  /** Island rect in window coordinates (origin top-left of the 720×320 window). */
  private islandRect(): { x: number; y: number; w: number; h: number } {
    const w = this.width.value;
    const hh = this.height.value;
    const viewportW = window.innerWidth > 0 ? window.innerWidth : PANEL_W;
    return { x: (viewportW - w) / 2, y: 0, w, h: hh };
  }

  // ── Window collapse (hidden → tiny wake strip, zero polling) ────────────────

  private updateWindowCollapsed() {
    if (this.collapseTimer != null) {
      window.clearTimeout(this.collapseTimer);
      this.collapseTimer = null;
    }
    if (State.mode === "hidden") {
      // Let the island finish retracting, then drop the window to the wake strip:
      // from there the OS delivers no cursor events, so nothing polls at all.
      this.collapseTimer = window.setTimeout(() => {
        this.collapseTimer = null;
        if (State.mode !== "hidden") return;
        this.collapsed = true;
        void Bridge.setCollapsed(true);
      }, 420);
    } else if (this.collapsed) {
      // Grow the window back before the island animates open.
      this.collapsed = false;
      void Bridge.setCollapsed(false);
    }
  }

  // ── Input ───────────────────────────────────────────────────────────────────

  private wireInput() {
    // The wake strip is the only thing the OS can hit while the island is hidden.
    this.wakeStrip.addEventListener("mouseenter", () => {
      Sound.resume();
      if (State.mode === "hidden") this.fsm.mouseEntered();
    });

    this.islandEl.addEventListener("mouseenter", () => {
      Sound.resume();
      if (State.mode === "hidden") this.fsm.mouseEntered();
    });

    this.islandEl.addEventListener("mousedown", (e) => {
      Sound.resume();
      State.lastActivity = performance.now();
      // A click makes a hover-opened island an ordinary open one.
      this.fsm.userInteracted();
      if (State.mode === "hidden") {
        this.fsm.mouseEntered();
        this.fsm.click();
        return;
      }
      // A press on Mochi may become a drag out to the desktop.
      if (e.button === 0 && this.isBotHit(e.clientX, e.clientY)) {
        this.botPress = { x: e.clientX, y: e.clientY };
      }
      // Right-click on Mochi opens the wardrobe, and closes it again.
      if (e.button === 2 && this.isBotHit(e.clientX, e.clientY)) {
        this.cancelBotHover();
        this.toggleWardrobe();
        return;
      }
      if (State.mode !== "expanded") {
        this.fsm.click();
        return;
      }
      if (this.isBotHit(e.clientX, e.clientY)) {
        this.cancelBotHover();
        this.engine.slap();
      }
    });

    // No browser menu over Mochi: his right-click is the wardrobe. Everywhere
    // else (the chat field) the webview keeps its own menu.
    this.islandEl.addEventListener("contextmenu", (e) => {
      if (this.isBotHit(e.clientX, e.clientY)) e.preventDefault();
    });

    // Dragging Mochi out of the island puts him on the desktop.
    window.addEventListener("mousemove", (e) => {
      if (this.desktop.carrying) {
        this.desktop.carry(e.clientX, e.clientY);
        return;
      }
      const press = this.botPress;
      if (!press) return;
      if (!(e.buttons & 1)) {
        this.botPress = null;
        return;
      }
      if (Math.hypot(e.clientX - press.x, e.clientY - press.y) <= DRAG_THRESHOLD) return;
      this.botPress = null;
      if (!this.canDragOut()) return;
      this.cancelBotHover();
      this.desktop.pickUp(e.clientX, e.clientY);
    });
    window.addEventListener("mouseup", (e) => {
      this.botPress = null;
      if (this.desktop.carrying) this.desktop.carryEnd(e.clientX, e.clientY);
    });

    // Only keys typed into the island itself land here, never Escape typed in
    // a terminal — so it may fold a waiting card away, as Escape in the notch
    // does on macOS.
    window.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && State.mode === "expanded") {
        if (State.pendingApproval) this.foldApproval();
        else if (!State.isPinned) this.collapse();
      }
      State.lastActivity = performance.now();
    });

    void onDragDrop((e) => this.onDragDrop(e));

    // Outside Tauri (plain browser) drive the cursor from DOM events so the
    // island can be inspected with `npm run dev`.
    if (!IS_TAURI) this.followPageCursor();
  }

  /**
   * Takes the cursor from the page's own mouse events instead of Rust's poll.
   * Used where the OS has no global cursor position (Wayland): the events only
   * fire while the pointer is over the island, so leaving the window is
   * reported as a cursor far away, which is what the poll would have said.
   */
  followPageCursor() {
    window.addEventListener("mousemove", (e) => this.onCursor(e.clientX, e.clientY));
    window.addEventListener("mouseout", (e) => {
      if (e.relatedTarget == null) this.onCursor(-10_000, -10_000);
    });
  }

  /**
   * Pointer on/off the island as the compositor sees it (Linux only). Null
   * until the first report, so a platform that never sends it is not gated.
   */
  private pointerInside: boolean | null = null;

  setPointerInside(inside: boolean) {
    this.pointerInside = inside;
  }

  /** Cursor in window-logical coordinates. */
  onCursor(x: number, y: number) {
    // WebKitGTK can deliver a mousemove after the pointer has left the layer
    // surface; trusting it re-enters the island and the auto-close never runs.
    if (this.pointerInside === false) {
      x = -10_000;
      y = -10_000;
    }
    State.mouse = { x, y };
    const rect = this.islandRect();
    State.mouseInIsland = { x: x - rect.x, y: y - rect.y };

    // Windows sends no cursor position with an OLE drag, so the drop sequence is
    // fed from the Win32 cursor poll instead — it runs throughout the drag.
    if (UploadSeq.isActive && !UploadSeq.dropped) {
      UploadSeq.updateCursor(State.mouseInIsland.x, State.mouseInIsland.y);
    }

    const inIsland =
      x >= rect.x - HIT_MARGIN && x <= rect.x + rect.w + HIT_MARGIN &&
      y >= rect.y - HIT_MARGIN && y <= rect.y + rect.h + HIT_MARGIN;

    // Recorded before the state machine hears of it: a transition it makes
    // right away (open on hover) reads where the pointer is, and must not see
    // the pointer as still outside.
    const wasIn = this.wasInIsland;
    this.wasInIsland = inIsland;
    if (inIsland && !wasIn) {
      if (this.fsm.state === "coucou") this.greeting.hover();
      this.fsm.mouseEntered();
    }
    if (!inIsland && wasIn) {
      this.fsm.mouseLeft();
    }

    // Bot hover → love
    const overBot = State.mode === "expanded" && State.stateOverride == null && this.isBotHit(x, y);
    if (overBot && !this.botHovering) this.botHoverIn(x, y);
    if (!overBot && this.botHovering) this.cancelBotHover();
    this.botHovering = overBot;
    if (this.botHovering) {
      const d = Math.hypot(x - this.botHoverStart.x, y - this.botHoverStart.y);
      if (d > 40) {
        this.botHoverStart = { x, y };
        this.scheduleLove();
      }
    }

    this.ensureRunning();
  }

  /** The greeting and the drop sequence draw a Mochi of their own: not that one. */
  private canDragOut(): boolean {
    if (State.mode === "hidden" || !this.desktop.canPickUp()) return false;
    return !(State.mode === "expanded" && (State.view === "greeting" || this.uploadActive));
  }

  private isBotHit(x: number, y: number): boolean {
    // Out on the desktop, the island's Mochi is invisible: nothing to hit.
    if (State.mochiOnDesktop) return false;
    const rect = this.islandRect();
    const cx = rect.x + this.botCx.value;
    const cy = rect.y + this.botCy.value;
    const radius = this.botSize.value / 2;
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius * radius;
  }

  private botHoverIn(x: number, y: number) {
    if (performance.now() / 1000 - this.lastLoveTime < 6) return;
    this.botHoverStart = { x, y };
    this.engine.blink();
    this.engine.tgEs = 1.08;
    Sound.play("hover");
    this.scheduleLove();
  }

  private scheduleLove() {
    if (this.botHoverTimer != null) window.clearTimeout(this.botHoverTimer);
    this.botHoverTimer = window.setTimeout(() => {
      this.botHoverTimer = null;
      if (!this.botHovering || State.stateOverride != null) return;
      if (performance.now() / 1000 - this.lastLoveTime < 6) return;
      this.lastLoveTime = performance.now() / 1000;
      this.engine.triggerEmote("love");
      Sound.play("love");
    }, 1900);
  }

  private cancelBotHover() {
    if (this.botHoverTimer != null) window.clearTimeout(this.botHoverTimer);
    this.botHoverTimer = null;
    this.engine.tgEs = 1;
  }

  /** Three slaps → dizzy + confused view for 3.3 s, then back. */
  handleDizzy() {
    this.prevViewBeforeConfused = State.view;
    State.stateOverride = "dizzy";
    this.engine.setState("dizzy");
    Sound.play("dizzy");
    this.alert("confused");
    if (this.confusedRecovery != null) window.clearTimeout(this.confusedRecovery);
    this.confusedRecovery = window.setTimeout(() => {
      this.confusedRecovery = null;
      State.stateOverride = null;
      this.engine.setState(State.effectiveState);
      if (State.view === "confused") {
        const fallback = State.defaultView();
        this.setView(this.prevViewBeforeConfused === "confused" ? fallback : this.prevViewBeforeConfused);
      }
      this.engine.triggerEmote("happy");
    }, 3300);
  }

  // ── Frame loop ──────────────────────────────────────────────────────────────

  ensureRunning() {
    if (this.running) return;
    this.running = true;
    this.lastFrame = performance.now();
    requestAnimationFrame(this.frame);
  }

  private frame = (nowMs: number) => {
    const dt = Math.min(0.05, (nowMs - this.lastFrame) / 1000);
    this.lastFrame = nowMs;

    this.width.step(dt, nowMs);
    this.height.step(dt, nowMs);
    this.radius.step(dt, nowMs);
    this.applyGeometry();

    if (this.dirty) {
      this.dirty = false;
      this.syncDom();
    }

    this.updateBotTargets();
    this.botCx.step(dt);
    this.botCy.step(dt);
    this.botSize.step(dt);

    const greetingActive = State.mode === "expanded" && State.view === "greeting";
    const away = State.mochiOnDesktop;
    const bluetoothCompact = State.mode === "compact" && (State.bluetoothStatus.connected || this.bluetoothDisconnectedMode);
    const downloadCompact = State.mode === "compact" && !bluetoothCompact && State.downloadStatus.active;
    const chargingCompact = State.mode === "compact" && !bluetoothCompact && !downloadCompact &&
      ((State.isCharging && (State.settings.chargingStatusInIsland ?? true)) || this.batteryBannerMode != null);
    const botVisible = !away && !chargingCompact && !bluetoothCompact && !downloadCompact;

    if (greetingActive) {
      const gctx = this.greetingCanvas.getContext("2d");
      if (gctx) {
        const dpr = Math.min(2, window.devicePixelRatio || 1);
        gctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        this.greeting.draw(gctx);
      }
    } else {
      // Kept running even while the drop canvas is up, so the island's own Mochi
      // is already in the right place the moment the canvas fades out.
      this.drawBot(dt);
    }

    const uploadActive = this.uploadActive;
    if (uploadActive) this.uploadCanvas.draw(UploadSeq.frame(), nowMs / 1000);
    this.uploadCanvas.el.classList.toggle("on", uploadActive);
    this.viewsEl.classList.toggle("hidden-by-upload", uploadActive);

    tickMiniBots(dt);
    // A ticker scroll that loses its frames freezes mid-way, rows overlapping.
    const viewAnimating = this.views.get(State.view)?.tick?.(nowMs) === true;
    if (UploadSeq.isActive) this.stepSequence();
    this.updateCountdown(nowMs);

    // Nothing is drawn while the island is hidden, so nothing may keep the loop
    // alive either. This used to read `... || this.engine.busy || State.mode !==
    // "hidden"`, and engine.busy is permanently true for any state with a
    // looping animation — breathing, ratelimit sweat, sleeping z's, the search
    // sweep — so a hidden island went on burning frames in exactly the states it
    // spends most of its life in. Geometry still has to finish retracting.
    const settling =
      this.width.animating || this.height.animating || this.radius.animating;
    const botActive = (botVisible || this.uploadActive) && this.engine.busy;
    const busy = State.mode === "hidden"
      ? settling
      : settling ||
        !this.botCx.settled || !this.botCy.settled || !this.botSize.settled ||
        greetingActive || botActive || UploadSeq.isActive || viewAnimating;

    if (busy) {
      requestAnimationFrame(this.frame);
    } else {
      this.running = false;
      Sound.idle();
      if (State.mode === "hidden") {
        void Bridge.trimMemory();
      }
    }
  };

  private updateBotTargets() {
    const p = botPosition(State.mode, State.view, this.height.value, State.uploadProgress);
    this.botCx.target = p.cx;
    this.botCy.target = p.cy;
    this.botSize.target = p.diameter / 0.6;

    const greetingActive = State.mode === "expanded" && State.view === "greeting";
    // The drop canvas draws its own Mochi; two of them would overlap. Out on the
    // desktop, he isn't here at all.
    const away = State.mochiOnDesktop;
    const bluetoothCompact = State.mode === "compact" && (State.bluetoothStatus.connected || this.bluetoothDisconnectedMode);
    const downloadCompact = State.mode === "compact" && !bluetoothCompact && State.downloadStatus.active;
    const chargingCompact = State.mode === "compact" && !bluetoothCompact && !downloadCompact &&
      ((State.isCharging && (State.settings.chargingStatusInIsland ?? true)) || this.batteryBannerMode != null);
    const visible = p.opacity > 0 && !greetingActive && !this.uploadActive && !away && !chargingCompact && !bluetoothCompact && !downloadCompact;
    this.botCanvas.style.opacity = visible ? "1" : "0";

    if (State.mode === "expanded" && State.view !== "uploading" && !greetingActive && !this.uploadActive && !away) {
      const d = p.diameter;
      const color = botGlowColor(State.effectiveState);
      this.botGlow.style.display = "block";
      this.botGlow.style.width = `${d * 2.2}px`;
      this.botGlow.style.height = `${d * 2.2}px`;
      this.botGlow.style.left = `${this.botCx.value - d * 1.1}px`;
      this.botGlow.style.top = `${this.botCy.value - d * 1.1}px`;
      this.botGlow.style.background = `radial-gradient(circle, ${color} 0%, transparent 62%)`;
      this.botGlow.style.opacity = String(botGlowOpacity(State.effectiveState));
    } else {
      this.botGlow.style.display = "none";
    }
  }

  private drawBot(dt: number) {
    const size = this.botSize.value;
    const w = Math.max(1, Math.round(size));
    const hCss = w + BOT_OVERHANG;
    const wCss = w + BOT_SIDE * 2;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    if (this.canvasPx !== w) {
      this.canvasPx = w;
      this.botCanvas.width = Math.round(wCss * dpr);
      this.botCanvas.height = Math.round(hCss * dpr);
      this.botCanvas.style.width = `${wCss}px`;
      this.botCanvas.style.height = `${hCss}px`;
    }
    this.botCanvas.style.left = `${this.botCx.value - wCss / 2}px`;
    this.botCanvas.style.top = `${this.botCy.value - BOT_OVERHANG / 2 - hCss / 2}px`;

    const ctx = this.botCanvas.getContext("2d");
    if (!ctx) return;

    const focus = State.focusTask;
    // While a plan card is open Mochi wears the plan's colour, like its pill.
    this.engine.bodyColor = planCardOpen()
      ? hexToRGB(openPlanColor())
      : focus?.isIntegration
        ? hexToRGB(focus.color)
        : null;
    this.engine.particleOverhang = BOT_OVERHANG;
    this.engine.lookX = this.lookX();
    this.engine.lookY = this.lookY();
    if (this.engine.morph > 0.3) {
      this.engine.slotHTarget = State.fileDragOver ? 0.2 : 0;
    } else {
      this.engine.slotHTarget = 0;
      if (this.engine.morph < 0.05) {
        this.engine.slotH = 0;
        this.engine.slotHVel = 0;
      }
    }
    // Only the main Mochi is dressed — the one of the main tool's pill (Settings →
    // Active pills): a focused integration pill shows its own colours, unless
    // the wardrobe is open (BotCanvasView.showOutfit, macOS).
    // In the wardrobe the hovered outfit swaps in at once, without the drop-in.
    const inWardrobe = State.mode === "expanded" && State.view === "wardrobe";
    const mainFocused = State.focusId == null || State.focusId === State.mainPillId;
    const showOutfit = mainFocused || State.mode !== "expanded" || inWardrobe;
    const outfit = State.wardrobePreview ?? this.seasons.get(parseOutfit(State.settings.mochiOutfit));
    this.engine.setOutfit(showOutfit ? outfit : "none", !inWardrobe);
    // Dances while music plays: always in the compact island, expanded only on
    // the music pill's card (BotCanvasView, macOS). Asked every frame.
    this.engine.setDancing(islandDances({
      music: State.spotifyPlaying,
      state: State.effectiveState,
      mode: State.mode,
      view: State.view,
      focusId: State.focusTask?.id,
    }));

    this.engine.update(dt);
    ctx.setTransform(dpr, 0, 0, dpr, BOT_SIDE * dpr, 0);
    ctx.clearRect(-BOT_SIDE, 0, wCss, hCss);
    ctx.save();
    this.engine.applyDance(ctx, w, hCss);
    this.engine.draw(ctx, w, hCss);
    ctx.restore();
  }

  /** BotCanvasView.lookX / lookY — tanh of the distance to the bot. */
  private lookX(): number {
    const rect = this.islandRect();
    const botScreenX = rect.x + this.botCx.value;
    return Math.tanh((State.mouse.x - botScreenX) / 260);
  }

  private lookY(): number {
    return -Math.tanh((State.mouse.y - this.botCy.value) / 200);
  }

  private updateCountdown(nowMs: number) {
    // The state machine's own deadline, so the bar follows an auto-close delay
    // edited while the countdown runs.
    const dueAt = this.fsm.homeCollapseDueAt;
    if (State.mode !== "expanded" || State.isPinned || dueAt == null) {
      this.countdown.style.width = "0px";
      return;
    }
    const autoClose = this.fsm.homeToPetitDelay;
    const windowS = Math.min(10, autoClose * 0.6);
    const remaining = (dueAt - nowMs) / 1000;
    this.countdown.style.width =
      remaining < windowS ? `${Math.max(0, clamp(remaining / windowS, 0, 1) * 160)}px` : "0px";
  }

  // ── DOM sync ────────────────────────────────────────────────────────────────

  private syncDom() {
    const expanded = State.mode === "expanded";
    const greetingActive = expanded && State.view === "greeting";

    const live = expanded && !greetingActive;
    this.contentEl.style.opacity = live ? "1" : "0";
    // While the drop sequence owns the body its buttons are painted on the canvas
    // underneath, so only the header may keep taking clicks up here.
    this.contentEl.style.pointerEvents = live && !this.uploadActive ? "auto" : "none";
    this.header.el.style.pointerEvents = live ? "auto" : "none";
    this.greetingCanvas.style.display = greetingActive ? "block" : "none";

    // Leaving the greeting, however it ends, lets its sound fade out.
    if (this.greetingShown && !greetingActive) this.greeting.leave();
    this.greetingShown = greetingActive;
    // A wardrobe try-on never outlives the wardrobe.
    if (State.wardrobePreview && !(expanded && State.view === "wardrobe")) State.wardrobePreview = null;

    this.header.sync();
    for (const [name, view] of this.views) {
      const on = name === State.view;
      view.el.classList.toggle("on", on);
      if (on) view.sync();
    }

    // The chat is the only view with a text field, so it is the only time the
    // island is allowed to take keyboard focus.
    if (this.lastSyncedView !== State.view) {
      const wasChat = this.lastSyncedView === "prompt";
      this.lastSyncedView = State.view;
      if (State.view === "prompt") {
        void Bridge.focusWindow(true);
        window.setTimeout(() => this.views.get("prompt")?.focus?.(), 120);
      } else if (wasChat) {
        void Bridge.focusWindow(false);
      }
    }

    // Compact mini grid, charging banner, bluetooth banner & download banner
    const bluetoothCompact = State.mode === "compact" && (State.bluetoothStatus.connected || this.bluetoothDisconnectedMode);
    const downloadCompact = State.mode === "compact" && !bluetoothCompact && State.downloadStatus.active;
    const chargingCompact = State.mode === "compact" && !bluetoothCompact && !downloadCompact &&
      ((State.isCharging && (State.settings.chargingStatusInIsland ?? true)) || this.batteryBannerMode != null);
    const musicCompact = State.mode === "compact" && !bluetoothCompact && !downloadCompact && !chargingCompact && State.spotifyPlaying;
    const showGrid = State.mode === "compact" && !chargingCompact && !bluetoothCompact && !downloadCompact && !musicCompact;
    this.miniGrid.style.opacity = showGrid ? "1" : "0";
    if (showGrid) {
      const others = State.otherTasks.slice(0, 4);
      const key = others.map((t) => t.id).join("|");
      if (this.miniGrid.dataset.key !== key) {
        this.miniGrid.dataset.key = key;
        this.miniGrid.replaceChildren();
        for (const t of others) {
          this.miniGrid.append(createMiniBot(t, 13));
        }
        pruneMiniBots();
      }
    }

    if (bluetoothCompact) {
      this.bluetoothBanner.style.display = "flex";
      const isDisconnected = this.bluetoothDisconnectedMode || !State.bluetoothStatus.connected;
      this.bluetoothBanner.classList.toggle("disconnected", isDisconnected);
      if (isDisconnected) {
        const devName = this.lastDisconnectedName || State.bluetoothStatus.name || "Bluetooth Device";
        this.bluetoothTitle.textContent = devName;
        this.bluetoothSub.textContent = t("Disconnected");
        const circumference = 87.96;
        this.bluetoothProgressCircle.style.strokeDashoffset = `${circumference}`;
        this.bluetoothProgressCircle.style.strokeDasharray = `${circumference}`;
      } else {
        const devName = State.bluetoothStatus.name || "Bluetooth Device";
        this.bluetoothTitle.textContent = devName;
        const lvl = State.bluetoothStatus.level;
        const circumference = 87.96;
        if (lvl != null) {
          this.bluetoothSub.textContent = t("Connected · {detail}", { detail: `${lvl}%` });
          const offset = circumference - (Math.max(0, Math.min(100, lvl)) / 100) * circumference;
          this.bluetoothProgressCircle.style.strokeDashoffset = `${offset}`;
          this.bluetoothProgressCircle.style.strokeDasharray = `${circumference}`;
        } else {
          this.bluetoothSub.textContent = t("Connected");
          this.bluetoothProgressCircle.style.strokeDashoffset = "0";
          this.bluetoothProgressCircle.style.strokeDasharray = `${circumference}`;
        }
      }
    } else {
      this.bluetoothBanner.style.display = "none";
    }

    if (downloadCompact) {
      this.downloadBanner.style.display = "flex";
      const isComplete = State.downloadCompleteFile != null;
      if (isComplete) {
        this.downloadTitle.textContent = "✓ Unduhan Selesai";
        this.downloadSub.textContent = State.downloadCompleteFile || "";
        this.downloadPercent.textContent = "100%";
        this.downloadProgressCircle.style.strokeDashoffset = "0";
        this.downloadProgressCircle.classList.remove("indeterminate");
      } else {
        const rawName = State.downloadStatus.filename;
        this.downloadTitle.textContent = (rawName && !rawName.startsWith("Unconfirmed "))
          ? rawName
          : "Mengunduh file...";

        const speed = State.downloadStatus.speedBytesPerSec;
        let speedStr = "0 KB/s";
        if (speed >= 1024 * 1024) {
          speedStr = `${(speed / (1024 * 1024)).toFixed(1)} MB/s`;
        } else if (speed >= 1024) {
          speedStr = `${(speed / 1024).toFixed(0)} KB/s`;
        } else if (speed > 0) {
          speedStr = `${speed} B/s`;
        }

        const pct = State.downloadStatus.percent;
        const downloaded = State.downloadStatus.downloadedBytes;
        let downloadedStr = "0 B";
        if (downloaded >= 1024 * 1024 * 1024) {
          downloadedStr = `${(downloaded / (1024 * 1024 * 1024)).toFixed(1)} GB`;
        } else if (downloaded >= 1024 * 1024) {
          downloadedStr = `${(downloaded / (1024 * 1024)).toFixed(1)} MB`;
        } else if (downloaded >= 1024) {
          downloadedStr = `${(downloaded / 1024).toFixed(0)} KB`;
        } else if (downloaded > 0) {
          downloadedStr = `${downloaded} B`;
        }

        this.downloadSub.textContent = pct != null ? `${speedStr} · ${pct}%` : `${speedStr} · Mengunduh...`;
        this.downloadPercent.textContent = pct != null ? `${pct}%` : downloadedStr;

        const circumference = 87.96;
        if (pct != null) {
          const offset = circumference - (Math.max(0, Math.min(100, pct)) / 100) * circumference;
          this.downloadProgressCircle.style.strokeDashoffset = `${offset}`;
          this.downloadProgressCircle.classList.remove("indeterminate");
        } else {
          this.downloadProgressCircle.classList.add("indeterminate");
        }
      }
    } else {
      this.downloadBanner.style.display = "none";
    }

    if (chargingCompact) {
      this.chargingBanner.style.display = "flex";
      const lvl = State.batteryStatus.level;
      this.chargingPercent.textContent = `${lvl}%`;
      const isLow = this.batteryBannerMode === "low" || (!State.isCharging && lvl <= 20);
      this.chargingBanner.classList.toggle("low-battery", isLow);
      if (isLow) {
        this.chargingTitle.textContent = t("Low Battery");
        this.chargingSub.textContent = t("Connect charger");
        this.chargingRing.replaceChildren(svg(ICONS.batteryLow, 15, { fill: "#ef4444" }));
      } else {
        const fullyCharged = lvl >= 100;
        this.chargingTitle.textContent = fullyCharged ? t("Fully Charged") : t("Charging");
        this.chargingSub.textContent = t("Charger connected");
        this.chargingRing.replaceChildren(svg(ICONS.bolt, 15, { fill: "#22c55e" }));
      }
    } else {
      this.chargingBanner.style.display = "none";
    }

    // Privacy indicator dots
    const camActive = State.privacyStatus.cameraActive;
    const micActive = State.privacyStatus.micActive;
    this.privacyCameraDot.style.display = camActive ? "block" : "none";
    this.privacyMicDot.style.display = micActive ? "block" : "none";
    if (camActive) {
      const apps = State.privacyStatus.activeCameraApps.join(", ");
      this.privacyCameraDot.title = apps ? `Kamera aktif (${apps})` : "Kamera / Webcam aktif";
    }
    if (micActive) {
      const apps = State.privacyStatus.activeMicApps.join(", ");
      const muted = State.privacyStatus.micMuted;
      this.privacyMicDot.classList.toggle("muted", muted);
      this.privacyMicDot.title = (apps ? `Mikrofon aktif (${apps})` : "Mikrofon aktif") + (muted ? " (Muted - Klik untuk Unmute)" : " (Klik untuk Mute)");
    }

    // Live Audio Visualizer & Compact Running Lyrics
    if (musicCompact) {
      this.audioVisualizer.style.display = "flex";
      this.audioVisualizer.classList.toggle("paused", !State.spotifyPlaying);
      if (State.activeLyricLine) {
        this.compactLyrics.style.display = "flex";
        if (this.compactLyricsText.textContent !== State.activeLyricLine) {
          this.compactLyricsText.textContent = State.activeLyricLine;
          this.compactLyrics.classList.remove("pulse");
          void this.compactLyrics.offsetWidth;
          this.compactLyrics.classList.add("pulse");
        }
      } else {
        this.compactLyrics.style.display = "none";
      }
    } else {
      this.audioVisualizer.style.display = "none";
      this.compactLyrics.style.display = "none";
    }

    syncMiniBotStates(State.tasks);
    this.engine.setState(State.effectiveState);
  }

  /** Applies settings coming from Rust at boot. */
  applySettings() {
    Sound.setEnabled(State.settings.soundEnabled);
    Sound.setVolume(State.settings.soundVolume);
    this.fsm.homeToPetitDelay = State.settings.autoCloseInterval;
    this.fsm.openOnHover = State.settings.openOnHover;
    State.notify();
  }

  get panelSize() {
    return { w: PANEL_W, h: PANEL_H };
  }

  get chatHeight() {
    return chatPromptHeight(State.chatHistory.length);
  }
}
