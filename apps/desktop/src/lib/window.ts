/** Window controls. In a plain browser (dev, tests) these are no-ops and the buttons hide. */
import { getCurrentWindow } from '@tauri-apps/api/window';

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const win = {
  minimize: () => (inTauri ? getCurrentWindow().minimize() : Promise.resolve()),
  toggleMaximize: () => (inTauri ? getCurrentWindow().toggleMaximize() : Promise.resolve()),
  close: () => (inTauri ? getCurrentWindow().close() : Promise.resolve()),
  hide: () => (inTauri ? getCurrentWindow().hide() : Promise.resolve()),
};
