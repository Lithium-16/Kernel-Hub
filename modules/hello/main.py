"""Hello: the phase 0 test module."""

import asyncio
import os
import time

from kernel_sdk import ActionContext, Module

mod = Module()
state = {"greetings": 0, "started_at": time.time()}


@mod.status
def status() -> dict:
    return {"greetings": state["greetings"], "uptime_s": round(time.time() - state["started_at"], 1)}


@mod.action("greet.say")
def greet(ctx: ActionContext, name: str) -> dict:
    state["greetings"] += 1
    return {"message": f"Hello, {name}!", "count": state["greetings"]}


@mod.action("counter.reset")
def reset(ctx: ActionContext) -> dict:
    state["greetings"] = 0
    return {"greetings": 0}


@mod.action("slow.wait")
async def wait(ctx: ActionContext, seconds: float) -> dict:
    await asyncio.sleep(seconds)
    return {"waited": seconds}


@mod.action("debug.crash")
def crash(ctx: ActionContext) -> None:
    os._exit(3)


if __name__ == "__main__":
    mod.run()
