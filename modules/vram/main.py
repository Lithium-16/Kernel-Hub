"""VRAM: shares the GPU between LLMs, image and video generation, and games."""

from kernel_sdk import ActionContext, Module
from vram import Manager

mod = Module()
manager = Manager(mod.settings, emit=mod.emit, log=mod.log)
mod.background(manager.poll)


@mod.status
def status() -> dict:
    return manager.snapshot


@mod.action("vram.free_idle")
async def free_idle(ctx: ActionContext) -> dict:
    return await manager.free_idle()


@mod.action("vram.focus")
async def focus(ctx: ActionContext, app: str) -> dict:
    return await manager.focus(app)


@mod.action("app.free")
async def free_one(ctx: ActionContext, app: str) -> dict:
    return await manager.free_one(app)


@mod.action("auto.set")
def auto(ctx: ActionContext, on: bool) -> dict:
    return manager.set_auto(on)


if __name__ == "__main__":
    mod.run()
