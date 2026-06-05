"use client";

import { useState } from "react";

const KEY = "harness";
const DEFAULT_HARNESS = "claude-code";
export type Harness = "opencode" | "claude-code" | "codex";

const HARNESSES: readonly Harness[] = ["opencode", "claude-code", "codex"];

function normalizeHarness(value: string | null): Harness {
  return HARNESSES.includes(value as Harness) ? (value as Harness) : DEFAULT_HARNESS;
}

export function useHarness() {
  const [harness, setHarnessState] = useState<Harness>(() => {
    if (typeof window === "undefined") return DEFAULT_HARNESS;
    return normalizeHarness(localStorage.getItem(KEY));
  });

  const setHarness = (v: Harness) => {
    const next = normalizeHarness(v);
    localStorage.setItem(KEY, next);
    setHarnessState(next);
  };

  return [harness, setHarness] as const;
}

export function readHarness(): Harness {
  if (typeof window === "undefined") return DEFAULT_HARNESS;
  return normalizeHarness(localStorage.getItem(KEY));
}
