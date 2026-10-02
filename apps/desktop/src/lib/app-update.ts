/** Updates for the desktop app itself (the Rust side does the downloading and installing). */
import { invoke } from '@tauri-apps/api/core';
import { inTauri } from './window.ts';

export interface Available {
  build: number;
}

export async function appBuild(): Promise<number | null> {
  return inTauri ? invoke<number>('app_build') : null;
}

/** A newer build, or null when up to date. Throws with a readable reason. */
export async function checkForUpdate(): Promise<Available | null> {
  if (!inTauri) throw new Error('updates only work in the installed app');
  return invoke<Available | null>('update_check');
}

/** Downloads, verifies and starts the installer; the app then closes and reopens. */
export async function installUpdate(): Promise<number> {
  return invoke<number>('update_install');
}

const AUTO_KEY = 'kernel.appAutoUpdate';

/** Install new app builds by themselves at start (and every few hours). Off unless chosen. */
export function loadAutoUpdate(): boolean {
  try {
    return localStorage.getItem(AUTO_KEY) === '1';
  } catch {
    return false;
  }
}

export function saveAutoUpdate(on: boolean): void {
  try {
    localStorage.setItem(AUTO_KEY, on ? '1' : '0');
  } catch {
    // private mode etc.: it just won't be remembered
  }
}
