"""Roblox: watch the AFK client on this PC, and close, rejoin or relaunch it."""

from kernel_sdk import ActionContext, Module
from watcher import Watcher

mod = Module()
watcher = Watcher(mod.settings)
mod.background(watcher.poll)


@mod.status
def status() -> dict:
    return watcher.snapshot


@mod.action("client.rejoin")
def rejoin(ctx: ActionContext) -> dict:
    return watcher.rejoin()


@mod.action("client.relaunch")
async def relaunch(ctx: ActionContext) -> dict:
    return await watcher.relaunch()


@mod.action("window.show")
def show(ctx: ActionContext) -> dict:
    return watcher.set_window(True)


@mod.action("window.hide")
def hide(ctx: ActionContext) -> dict:
    return watcher.set_window(False)


@mod.action("client.close")
async def close(ctx: ActionContext) -> dict:
    return await watcher.close()


if __name__ == "__main__":
    mod.run()
