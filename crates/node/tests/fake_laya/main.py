"""A stand-in for LAYA: canned decisions, and a record of what it was asked."""

import json

from kernel_sdk import ActionContext, Module

mod = Module()
seen: list[dict] = []


@mod.status
def status() -> dict:
    return {"state": "ready", "seen": seen}


@mod.action("tools.rank")
def decide(ctx: ActionContext, text: str, context: str, options: str, keep: int, pick: int) -> dict:
    opts = list(json.loads(options))
    seen.append({"text": text, "context": context, "options": opts})
    # "how is" -> the status, "count" -> a button (both very sure), anything else -> unsure.
    first = {"how is": "status:hello", "count": "hello__greet_count"}
    top = next((t for words, t in first.items() if words in text), "hello__event_emit")
    ranked = [top] + [o for o in opts if o != top]
    return {
        "ranked": ranked[:keep],
        "top": top,
        "confidence": 0.3 if top == "hello__event_emit" else 0.97,
        "hard": 0.1,
        "shortlisted": False,
        "model": "fake",
        "ms": 1.0,
    }


if __name__ == "__main__":
    mod.run()
