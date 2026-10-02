"""LAYA: a fast decision model in front of the assistant (picks likely tools for a message)."""

from decider import Decider
from kernel_sdk import ActionContext, Module

mod = Module()
decider = Decider(mod.settings, emit=mod.emit, log=mod.log)
mod.background(decider.start)


@mod.status
def status() -> dict:
    return decider.snapshot()


@mod.action("tools.rank")
async def decide(ctx: ActionContext, text: str, context: str, options: str, keep: int, pick: int) -> dict:
    return await decider.decide(text, context, options, keep, pick)


@mod.action("model.load")
async def load(ctx: ActionContext) -> dict:
    return await decider.load()


@mod.action("model.unload")
def unload(ctx: ActionContext) -> dict:
    return decider.unload()


@mod.action("setup.install")
async def install(ctx: ActionContext) -> dict:
    return await decider.install()


if __name__ == "__main__":
    mod.run()
