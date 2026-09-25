import type { ReactNode } from 'react';
import { inTauri, win } from '../lib/window.ts';

export function TitleBar({ title, meta }: { title: string; meta?: ReactNode }) {
  return (
    <div className="titlebar">
      <div className="title" data-tauri-drag-region>
        <span className="name" data-tauri-drag-region>
          {title}
        </span>
        {meta ? (
          <span className="muted" data-tauri-drag-region>
            {meta}
          </span>
        ) : null}
      </div>
      {inTauri ? (
        <>
          <button
            type="button"
            className="win-btn"
            aria-label="Minimize"
            onClick={() => void win.minimize()}
          >
            <svg
              width="10"
              height="10"
              viewBox="0 0 10 10"
              stroke="currentColor"
              strokeWidth="1"
              aria-hidden="true"
            >
              <line x1="0" y1="5" x2="10" y2="5" />
            </svg>
          </button>
          <button
            type="button"
            className="win-btn"
            aria-label="Maximize"
            onClick={() => void win.toggleMaximize()}
          >
            <svg
              width="10"
              height="10"
              viewBox="0 0 10 10"
              fill="none"
              stroke="currentColor"
              strokeWidth="1"
              aria-hidden="true"
            >
              <rect x="0.5" y="0.5" width="9" height="9" />
            </svg>
          </button>
          <button
            type="button"
            className="win-btn close"
            aria-label="Close"
            onClick={() => void win.close()}
          >
            <svg
              width="10"
              height="10"
              viewBox="0 0 10 10"
              stroke="currentColor"
              strokeWidth="1"
              aria-hidden="true"
            >
              <line x1="0" y1="0" x2="10" y2="10" />
              <line x1="10" y1="0" x2="0" y2="10" />
            </svg>
          </button>
        </>
      ) : null}
    </div>
  );
}
