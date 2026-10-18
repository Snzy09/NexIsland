// Charging banner & status tests (src/core/layout.ts, src/core/state.ts, src/i18n/extra.json).

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  COMPACT_CHARGING_H,
  COMPACT_CHARGING_R,
  COMPACT_CHARGING_W,
  COMPACT_W,
  NOTCH_H,
  MINIMIZED_PILL_W,
  MINIMIZED_PILL_H,
  MINIMIZED_PILL_R,
  WAKE_STRIP_H,
  islandSize,
} from "../src/core/layout.ts";
import { State, DEFAULT_SETTINGS } from "../src/core/state.ts";
import extra from "../src/i18n/extra.json" with { type: "json" };

test("minimized pill dimensions when hidden", () => {
  assert.equal(MINIMIZED_PILL_W, 140);
  assert.equal(MINIMIZED_PILL_H, 12);
  assert.equal(MINIMIZED_PILL_R, 6);
  assert.equal(WAKE_STRIP_H, 20);

  const hiddenSize = islandSize("hidden", "overview", 0, false);
  assert.deepEqual(hiddenSize, { w: MINIMIZED_PILL_W, h: MINIMIZED_PILL_H });
});

test("compact island dimensions with and without charging", () => {
  assert.equal(COMPACT_CHARGING_W, 330);
  assert.equal(COMPACT_CHARGING_H, 50);
  assert.equal(COMPACT_CHARGING_R, 16);

  const normalCompact = islandSize("compact", "overview", 0, false);
  assert.deepEqual(normalCompact, { w: COMPACT_W, h: NOTCH_H });

  const chargingCompact = islandSize("compact", "overview", 0, true);
  assert.deepEqual(chargingCompact, { w: COMPACT_CHARGING_W, h: COMPACT_CHARGING_H });
});

test("State tracks battery status and isCharging getter", () => {
  State.batteryStatus = { charging: false, level: 85, hasBattery: true };
  assert.equal(State.isCharging, false);

  State.batteryStatus = { charging: true, level: 26, hasBattery: true };
  assert.equal(State.isCharging, true);

  assert.equal(DEFAULT_SETTINGS.chargingStatusInIsland, true);
});

test("charging translations exist in extra.json for all supported languages", () => {
  const languages = ["zh-Hans", "hi", "es", "ar", "fr", "bn", "pt-BR", "ru", "id"];
  const keys = [
    "Charging",
    "Charger connected",
    "Fully Charged",
    "Charging banner in minimized island",
    "Shows battery percentage and charging indicator in the island when minimized.",
  ];

  for (const key of keys) {
    const entry = extra.strings[key];
    assert.ok(entry, `missing translation entry for "${key}"`);
    for (const lang of languages) {
      assert.ok(entry[lang], `missing language "${lang}" for "${key}"`);
      assert.ok(entry[lang].length > 0, `empty translation for "${lang}" in "${key}"`);
    }
  }
});
