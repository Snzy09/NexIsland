// The Spotify pill and card — DOM ports of SpotifyPill / SpotifyCardView
// (SpotifyViews.swift) and the shared now-playing pieces (NowPlayingViews.swift
// and MusicControlButton). Sizes, colours and wording are the Mac's.
//
// What they show comes from src-tauri/src/spotify.rs (MPRIS on Linux, the
// system media controls on Windows) through
// island/spotify.ts. A click changes the page's copy at once and Spotify
// confirms it, as the Mac's controller does.

import { h, svg, clear, dot } from "./dom";
import { ICONS } from "./icons";
import { Bridge } from "../core/bridge";
import { State, type AgentTask } from "../core/state";
import {
  SPOTIFY_GREEN, SPOTIFY_ID, Spotify, currentArtwork, formatTime, isAd, spotifyPosition, volumeLevel, withPlaying,
  findActiveLyric, parseLrc,
  type LyricLine, type SpotifyLyrics, type SpotifyTrack,
} from "../core/spotify";
import { createMiniBot } from "../mochi/minibots";
import { pillDefinition } from "../core/pills";
import { N_, t } from "../i18n/i18n";


function lighter(hex: string, amount: number): string {
  const v = parseInt(hex.replace("#", ""), 16);
  const c = [(v >> 16) & 255, (v >> 8) & 255, v & 255].map((x) => Math.min(255, Math.round(x + amount * 255)));
  return `rgb(${c[0]},${c[1]},${c[2]})`;
}

// ── Controls (shared by the pill and the card) ───────────────────────────────

/** Play/pause, at once on the page, then in Spotify (SpotifyController.playPause). */
export function togglePlay() {
  const s = Spotify.state;
  if (!s.running) return;
  Spotify.state = withPlaying(s, !s.playing, Date.now());
  State.notify();
  void Bridge.spotifyControl("playPause");
}

function seek(seconds: number) {
  const s = Spotify.state;
  const track = s.track;
  if (!s.running || !track || isAd(track)) return;
  const target = Math.min(Math.max(0, seconds), Math.max(0, track.duration - 1));
  Spotify.state = { ...s, position: target, positionAt: Date.now() };
  State.notify();
  void Bridge.spotifyControl("seek", target);
}

function setFlag(which: "shuffle" | "repeat", on: boolean) {
  if (!Spotify.state.running) return;
  Spotify.state = { ...Spotify.state, [which]: on };
  State.notify();
  void Bridge.spotifyControl(which, on ? 1 : 0);
}

// A slider drag sends at most one volume every 120 ms (SpotifyController.setVolume).
let pendingVolume: number | null = null;
let volumeTimer: number | null = null;

function setVolume(value: number) {
  if (!Spotify.state.running) return;
  const v = Math.min(100, Math.max(0, Math.round(value)));
  Spotify.state = { ...Spotify.state, volume: v };
  State.notify();
  pendingVolume = v;
  if (volumeTimer != null) return;
  const send = () => {
    const next = pendingVolume;
    pendingVolume = null;
    if (next == null) {
      volumeTimer = null;
      return;
    }
    void Bridge.spotifyControl("volume", next);
    volumeTimer = window.setTimeout(send, 120);
  };
  send();
}

// ── Pill (overview right column) ──────────────────────────────────────────────

export interface SpotifyPillHost {
  el: HTMLElement;
  sync(): void;
}

/** MusicControlButton: a 20 px round button in the pill's colour. */
function pillButton(color: string, icon: string, onClick: () => void): HTMLElement {
  const b = h("button", { class: "np-pill-btn" }, svg(icon, 8));
  b.style.borderColor = `${color}24`;
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    onClick();
  });
  b.addEventListener("mouseenter", () => {
    b.style.background = `${color}2e`;
    b.style.borderColor = `${color}8c`;
    b.style.color = lighter(color, 0.3);
    b.style.boxShadow = `0 0 6px ${color}59`;
  });
  b.addEventListener("mouseleave", () => {
    b.style.background = "";
    b.style.borderColor = `${color}24`;
    b.style.color = "";
    b.style.boxShadow = "";
  });
  return b;
}

/**
 * SpotifyPill: the agent pill's look, with a mini Mochi that dances while
 * Spotify plays, and play/pause + next on hover when a track is loaded.
 */
export function buildSpotifyPill(task: AgentTask, onTap: () => void): SpotifyPillHost {
  const color = task.color;
  const mini = createMiniBot(task, 24, () => State.spotifyPlaying);
  const label = h("span", { class: "lbl", text: task.name });
  const playBtn = pillButton(color, ICONS.play, togglePlay);
  const nextBtn = pillButton(color, ICONS.forward, () => void Bridge.spotifyControl("next"));
  const controls = h("div", { class: "np-pill-controls" }, playBtn, nextBtn);
  const pill = h("div", { class: "pill np-pill", onclick: onTap }, mini, label, controls);
  pill.style.borderColor = `${color}24`;
  let hovered = false;
  let playing: boolean | null = null;

  const sync = () => {
    const s = Spotify.state;
    const current = State.tasks.find((x) => x.id === task.id);
    const name = current?.name ?? task.name;
    if (label.textContent !== name) label.textContent = name;
    const show = hovered && s.track != null;
    pill.classList.toggle("controls", show);
    if (playing !== s.playing) {
      playing = s.playing;
      clear(playBtn);
      playBtn.append(svg(s.playing ? ICONS.pause : ICONS.play, 8));
    }
    playBtn.title = s.playing ? t("Pause") : t("Play");
    nextBtn.title = t("Next");
  };

  pill.addEventListener("mouseenter", () => {
    hovered = true;
    pill.style.background = `${color}2e`;
    pill.style.borderColor = `${color}8c`;
    pill.style.boxShadow = `0 2px 10px ${color}59`;
    label.style.color = lighter(color, 0.3);
    sync();
  });
  pill.addEventListener("mouseleave", () => {
    hovered = false;
    pill.style.background = "";
    pill.style.borderColor = `${color}24`;
    pill.style.boxShadow = "";
    label.style.color = "";
    sync();
  });
  sync();
  return { el: pill, sync };
}

// ── Card (overview left card) ─────────────────────────────────────────────────

/**
 * NowPlayingBar: a grey track whose filled part turns green and grows a knob
 * under the pointer; drag or click anywhere on it.
 */
class NowPlayingBar {
  readonly el: HTMLElement;
  private fill: HTMLElement;
  private knob: HTMLElement;
  private hovering = false;
  private dragging = false;
  private fraction = 0;
  enabled = true;
  private onChange: (f: number) => void;
  private onCommit: (f: number) => void;

  constructor(onChange: (f: number) => void, onCommit: (f: number) => void) {
    this.onChange = onChange;
    this.onCommit = onCommit;
    this.fill = h("i", { class: "np-fill" });
    this.knob = h("i", { class: "np-knob" });
    this.fill.style.background = "#C5C8CD";
    this.el = h("div", { class: "np-bar" }, h("i", { class: "np-track" }), this.fill, this.knob);
    this.el.addEventListener("mouseenter", () => {
      this.hovering = true;
      this.paint();
    });
    this.el.addEventListener("mouseleave", () => {
      this.hovering = false;
      this.paint();
    });
    this.el.addEventListener("pointerdown", (e) => {
      if (!this.enabled || e.button !== 0) return;
      e.stopPropagation();
      this.dragging = true;
      try {
        this.el.setPointerCapture(e.pointerId);
      } catch {
        /* capture is a nicety */
      }
      this.onChange(this.at(e.clientX));
      this.paint();
    });
    this.el.addEventListener("pointermove", (e) => {
      if (this.dragging) this.onChange(this.at(e.clientX));
    });
    const end = (e: PointerEvent) => {
      if (!this.dragging) return;
      this.dragging = false;
      this.onCommit(this.at(e.clientX));
      this.paint();
    };
    this.el.addEventListener("pointerup", end);
    this.el.addEventListener("pointercancel", end);
  }

  get active(): boolean {
    return this.enabled && (this.hovering || this.dragging);
  }

  private at(clientX: number): number {
    const r = this.el.getBoundingClientRect();
    return Math.min(1, Math.max(0, (clientX - r.left) / Math.max(1, r.width)));
  }

  set(fraction: number) {
    this.fraction = Math.min(1, Math.max(0, fraction));
    this.paint();
  }

  private paint() {
    const active = this.active;
    this.el.classList.toggle("active", active);
    const pct = `${this.fraction * 100}%`;
    this.fill.style.width = pct;
    this.fill.style.background = active ? SPOTIFY_GREEN : "#C5C8CD";
    this.knob.style.left = `clamp(0px, calc(${pct} - 4.5px), calc(100% - 9px))`;
  }
}

/** NowPlayingIconButton: an icon that brightens under the pointer. */
function iconButton(icon: string, size: number, stroke: number, onClick: () => void): HTMLElement {
  const b = h("button", { class: "np-icon" }, svg(icon, size, stroke ? { stroke } : {}));
  b.addEventListener("click", onClick);
  return b;
}

export interface SpotifyCardHost {
  el: HTMLElement;
  sync(): void;
}

/** SpotifyCardView: now playing, or the idle card (not playing / not installed). */
export function buildSpotifyCard(): SpotifyCardHost {
  const green = SPOTIFY_GREEN;

  // Now playing: artwork row, progress row, controls row.
  const art = h("div", { class: "np-art" });
  const artImg = h("img", { alt: "", draggable: "false" }) as HTMLImageElement;
  const artNote = svg(ICONS.musicNote, 16);
  artNote.style.color = `${green}b3`;
  art.append(artNote, artImg);
  art.addEventListener("click", () => void Bridge.spotifyOpen());
  const title = h("span", { class: "np-title" });
  const subtitle = h("div", { class: "np-sub" });
  const openChevron = h("button", {
    class: "np-chevron-btn",
    title: t("Open Spotify"),
    onclick: (e) => {
      e.stopPropagation();
      void Bridge.spotifyOpen();
    },
  }, svg(ICONS.chevronRight ?? ICONS.forward, 8));
  const head = h("div", { class: "np-head" },
    art,
    h("div", { class: "np-text" }, h("div", { class: "np-title-row" }, dot(green, 6), title), subtitle),
    openChevron,
  );

  let dragFraction: number | null = null;
  const elapsed = h("span", { class: "np-time elapsed" });
  const remaining = h("span", { class: "np-time remaining" });
  const progress = new NowPlayingBar(
    (f) => {
      dragFraction = f;
      paintProgress();
    },
    (f) => {
      const d = Spotify.state.track?.duration ?? 0;
      dragFraction = null;
      seek(f * d);
    },
  );
  const progressRow = h("div", { class: "np-progress" }, elapsed, progress.el, remaining);

  const shuffle = iconButton(ICONS.shuffle, 10, 2.2, () => setFlag("shuffle", !Spotify.state.shuffle));
  const prev = iconButton(ICONS.backward, 11, 0, () => void Bridge.spotifyControl("previous"));
  const next = iconButton(ICONS.forward, 11, 0, () => void Bridge.spotifyControl("next"));
  const repeat = iconButton(ICONS.repeat, 10, 2.2, () => setFlag("repeat", !Spotify.state.repeat));
  prev.style.color = "#C5C8CD";
  next.style.color = "#C5C8CD";
  const play = h("button", { class: "np-play", onclick: togglePlay });
  const volIcon = h("span", { class: "np-vol-icon" });
  const volume = new NowPlayingBar(
    (f) => setVolume(f * 100),
    (f) => setVolume(f * 100),
  );
  const volumeBox = h("div", { class: "np-volume" }, volIcon, volume.el);
  let lyricsOpen = false;
  let lyricsLoading = false;
  let lyricsData: SpotifyLyrics | null = null;
  let lyricLines: LyricLine[] = [];
  let lineElements: HTMLElement[] = [];
  let activeLyricIndex = -1;
  let userScrolled = false;
  let userScrollTimer: number | null = null;
  let lastLyricsTrackId: string | null = null;

  const lyricsBtn = iconButton(ICONS.lyrics, 11, 1.8, () => {
    lyricsOpen = !lyricsOpen;
    syncLyricsMode();
  });
  lyricsBtn.title = t("Lyrics");
  lyricsBtn.style.color = "#6B7079";

  const lyricsPreviewDot = dot(green, 4);
  const lyricsPreviewText = h("span", { text: t("Lyrics") });
  const lyricsPreview = h("div", {
    class: "np-lyrics-preview",
    onclick: () => {
      lyricsOpen = true;
      syncLyricsMode();
    },
  }, lyricsPreviewDot, lyricsPreviewText);
  lyricsPreview.title = t("Lyrics");

  const lyricsContainer = h("div", { class: "np-lyrics-container" });
  lyricsContainer.style.display = "none";
  lyricsContainer.addEventListener("scroll", () => {
    userScrolled = true;
    if (userScrollTimer != null) window.clearTimeout(userScrollTimer);
    userScrollTimer = window.setTimeout(() => {
      userScrolled = false;
      userScrollTimer = null;
    }, 3000);
  });

  // Date and status bar at the top (media_1792332593750_68fd78af.png)
  const dateRing = dot(green, 6);
  dateRing.classList.add("np-date-ring");
  const dateSpan = h("span", { class: "np-date-val" });
  const timeSpan = h("b", { class: "np-time-val" });
  const dateText = h("div", { class: "np-date-text" }, dateSpan, timeSpan);
  const menuBtn = h("button", {
    class: "np-menu-btn",
    title: t("Open Spotify"),
    onclick: (e) => {
      e.stopPropagation();
      void Bridge.spotifyOpen();
    },
  }, h("span", { class: "np-dots-icon", text: "•••" }));
  const dateBar = h("div", { class: "np-date-bar" },
    h("div", { class: "np-date-left" }, dateRing, dateText),
    menuBtn,
  );

  function syncDateBar() {
    const now = new Date();
    const days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    dateSpan.textContent = `${days[now.getDay()]} ${now.getDate()} ${months[now.getMonth()]} ${now.getFullYear()} `;
    timeSpan.textContent = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
  }
  syncDateBar();

  // Carousel dots at the bottom
  const dotsPill = h("span", { class: "np-dot-pill" });
  const dotsRow = h("div", { class: "np-dots-row" },
    dotsPill,
    h("span", { class: "np-dot" }),
    h("span", { class: "np-dot" }),
    h("span", { class: "np-dot" }),
    h("span", { class: "np-dot" }),
  );

  const controls = h("div", { class: "np-controls" },
    h("div", { class: "np-buttons" }, shuffle, prev, play, next, repeat, lyricsBtn),
    h("div", { class: "grow" }),
    volumeBox,
  );

  const leftPane = h("div", { class: "np-pane-left" },
    dateBar,
    head,
    progressRow,
    controls,
    lyricsPreview,
    dotsRow,
  );
  const divider = h("div", { class: "np-pane-divider" });
  divider.style.display = "none";
  const lyricsPane = h("div", { class: "np-pane-right" }, lyricsContainer);
  lyricsPane.style.display = "none";

  const playingEl = h("div", { class: "np-card" }, leftPane, divider, lyricsPane);

  // Idle: the same layout as the other idle cards.
  const idleDot = dot("#22C55E", 5);
  const idleText = h("span");
  const idleAction = h("button", { class: "link-btn", style: `color:${green}d9` });
  idleAction.addEventListener("click", () => void Bridge.spotifyOpen());
  const idleSub = h("span");
  const idleEl = h("div", { class: "int-card" },
    h("div", { class: "int-head" }, dot(green, 7), h("b", { text: "Spotify" }), idleSub),
    h("div", { class: "int-status" }, idleDot, idleText),
    h("div", { class: "int-actions" }, idleAction),
  );

  const el = h("div", { class: "np-root" });
  let showing: "playing" | "idle" | null = null;
  let shownTrack: SpotifyTrack | null = null;
  let playIcon: boolean | null = null;
  let volIconLevel = -1;
  let timer: number | null = null;

  function renderLyrics() {
    clear(lyricsContainer);
    lineElements = [];
    if (lyricsLoading) {
      lyricsContainer.append(h("div", { class: "np-lyrics-status", text: t("Loading lyrics…") }));
      return;
    }
    if (lyricsData?.instrumental) {
      lyricsContainer.append(h("div", { class: "np-lyrics-status", text: `♪ ${t("Instrumental")}` }));
      return;
    }
    if (lyricLines.length === 0) {
      lyricsContainer.append(h("div", { class: "np-lyrics-status", text: t("No lyrics available") }));
      return;
    }
    for (let i = 0; i < lyricLines.length; i++) {
      const line = lyricLines[i];
      const lineEl = h("div", {
        class: `np-lyrics-line${i === activeLyricIndex ? " active" : ""}`,
        text: line.text,
      });
      if (line.time >= 0) {
        lineEl.addEventListener("click", () => seek(line.time));
      }
      lineElements.push(lineEl);
      lyricsContainer.append(lineEl);
    }
  }

  function scrollActiveIntoView() {
    if (!lyricsOpen || userScrolled) return;
    if (activeLyricIndex >= 0 && lineElements[activeLyricIndex]) {
      const targetEl = lineElements[activeLyricIndex];
      const c = lyricsContainer;
      try {
        const target = targetEl.offsetTop - c.offsetTop - (c.clientHeight / 2) + (targetEl.clientHeight / 2);
        c.scrollTo?.({ top: Math.max(0, target), behavior: "smooth" });
      } catch {
        /* scrollTo optional */
      }
    }
  }

  function updateActiveLyric(pos: number) {
    if (lyricLines.length > 0 && lyricLines[0].time >= 0) {
      const next = findActiveLyric(lyricLines, pos);
      if (next !== activeLyricIndex) {
        activeLyricIndex = next;
        for (let i = 0; i < lineElements.length; i++) {
          lineElements[i].classList.toggle("active", i === activeLyricIndex);
        }
        if (activeLyricIndex >= 0 && lyricLines[activeLyricIndex]) {
          lyricsPreviewText.textContent = `♪ ${lyricLines[activeLyricIndex].text}`;
        } else {
          lyricsPreviewText.textContent = t("Lyrics");
        }
        scrollActiveIntoView();
      }
    } else if (lyricsData?.instrumental) {
      lyricsPreviewText.textContent = `♪ ${t("Instrumental")}`;
    } else if (lyricsLoading) {
      lyricsPreviewText.textContent = t("Loading lyrics…");
    } else {
      lyricsPreviewText.textContent = t("Lyrics");
    }
  }

  function fetchTrackLyrics(track: SpotifyTrack) {
    if (isAd(track) || !track.title) {
      lyricsData = null;
      lyricLines = [];
      lyricsLoading = false;
      renderLyrics();
      return;
    }
    lyricsLoading = true;
    lyricsData = null;
    lyricLines = [];
    activeLyricIndex = -1;
    renderLyrics();
    void Bridge.spotifyLyrics(track.artist || "", track.title, track.duration).then((data) => {
      if (shownTrack?.id !== track.id) return;
      lyricsLoading = false;
      lyricsData = data;
      if (data?.synced_lyrics) {
        lyricLines = parseLrc(data.synced_lyrics);
      } else if (data?.plain_lyrics) {
        lyricLines = data.plain_lyrics
          .split(/\r?\n/)
          .map((x) => x.trim())
          .filter((x) => x.length > 0)
          .map((text) => ({ time: -1, text }));
      } else {
        lyricLines = [];
      }
      renderLyrics();
      const s = Spotify.state;
      updateActiveLyric(spotifyPosition(s, Date.now()));
    });
  }

  function syncLyricsMode() {
    State.spotifyLyricsOpen = lyricsOpen;
    lyricsBtn.classList.toggle("active", lyricsOpen);
    lyricsBtn.style.color = lyricsOpen ? green : "#6B7079";
    lyricsContainer.style.display = lyricsOpen ? "" : "none";
    lyricsPane.style.display = lyricsOpen ? "" : "none";
    divider.style.display = lyricsOpen ? "" : "none";
    lyricsPreview.style.display = lyricsOpen ? "none" : "";
    dateBar.style.display = lyricsOpen ? "none" : "";
    dotsRow.style.display = lyricsOpen ? "none" : "";
    head.classList.toggle("compact", lyricsOpen);
    playingEl.classList.toggle("lyrics-expanded", lyricsOpen);
    if (lyricsOpen) {
      userScrolled = false;
      scrollActiveIntoView();
    }
    State.notify();
  }

  function paintProgress() {
    const s = Spotify.state;
    const d = s.track?.duration ?? 0;
    const pos = dragFraction != null ? dragFraction * d : spotifyPosition(s, Date.now());
    const f = dragFraction ?? (d > 0 ? Math.min(1, Math.max(0, pos / d)) : 0);
    progress.set(f);
    elapsed.textContent = formatTime(f * d);
    remaining.textContent = d > 0 ? `-${formatTime(Math.max(0, d - f * d))}` : "";
    updateActiveLyric(pos);
  }

  /** The position runs on its own clock, twice a second, only while it plays on screen. */
  function syncTimer() {
    const run = Spotify.state.playing && showing === "playing" && el.isConnected &&
      State.mode === "expanded" && State.view === "overview";
    if (run && timer == null) {
      timer = window.setInterval(() => {
        if (!(Spotify.state.playing && el.isConnected && State.mode === "expanded" && State.view === "overview")) {
          if (timer != null) window.clearInterval(timer);
          timer = null;
          return;
        }
        paintProgress();
      }, 500);
    } else if (!run && timer != null) {
      window.clearInterval(timer);
      timer = null;
    }
  }

  function syncPlaying(track: SpotifyTrack) {
    syncDateBar();
    const s = Spotify.state;
    if (shownTrack !== track) {
      shownTrack = track;
      title.textContent = isAd(track) ? t("Advertisement") : track.title;
      const sub = [track.artist, track.album].filter((x) => x).join(" · ");
      subtitle.textContent = sub;
      subtitle.style.display = sub ? "" : "none";
      if (track.id !== lastLyricsTrackId) {
        lastLyricsTrackId = track.id;
        fetchTrackLyrics(track);
      }
    } else if (isAd(track)) {
      title.textContent = t("Advertisement");
    }
    art.title = track.album ? t("{0} — open Spotify", { 0: track.album }) : t("Open Spotify");
    const cover = currentArtwork(s);
    if (cover) {
      if (artImg.getAttribute("src") !== cover) artImg.setAttribute("src", cover);
      art.classList.add("has-art");
    } else {
      art.classList.remove("has-art");
      artImg.removeAttribute("src");
    }

    progress.enabled = !isAd(track) && track.duration > 0;
    paintProgress();

    shuffle.style.color = s.shuffle ? green : "#6B7079";
    shuffle.title = s.shuffle ? t("Shuffle on") : t("Shuffle off");
    repeat.style.color = s.repeat ? green : "#6B7079";
    repeat.title = s.repeat ? t("Repeat on") : t("Repeat off");
    lyricsBtn.title = t("Lyrics");
    lyricsPreview.title = t("Lyrics");
    prev.title = t("Previous");
    next.title = t("Next");
    if (playIcon !== s.playing) {
      playIcon = s.playing;
      clear(play);
      const icon = svg(s.playing ? ICONS.pause : ICONS.play, 9);
      if (!s.playing) icon.style.transform = "translateX(1px)";
      play.append(icon);
    }
    play.title = s.playing ? t("Pause") : t("Play");

    const level = volumeLevel(s.volume);
    if (level !== volIconLevel) {
      volIconLevel = level;
      clear(volIcon);
      volIcon.append(svg([ICONS.volume0, ICONS.volume1, ICONS.volume2, ICONS.volume3][level], 10, { stroke: 2.2 }));
    }
    volume.set(s.volume / 100);
    volumeBox.title = t("Volume {0}%", { 0: s.volume });
  }

  function syncIdle() {
    const installed = Spotify.state.installed || Spotify.state.running;
    idleDot.style.background = installed ? "#22C55E" : "#F4505E";
    idleText.textContent = installed ? t("Not playing") : t("Spotify not installed");
    idleAction.textContent = installed ? t("Open Spotify") : t("Get Spotify");
    idleSub.textContent = t(pillDefinition(SPOTIFY_ID)?.subtitle ?? N_("Integration"));
  }

  return {
    el,
    sync() {
      const track = Spotify.state.track;
      const want = track ? "playing" : "idle";
      if (want !== showing) {
        showing = want;
        clear(el);
        el.append(want === "playing" ? playingEl : idleEl);
        shownTrack = null;
      }
      if (track) syncPlaying(track);
      else syncIdle();
      syncTimer();
    },
  };
}
