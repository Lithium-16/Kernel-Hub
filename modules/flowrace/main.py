"""Flow Race: the puzzle racing game, always up on this PC and shared through Tailscale."""

from flowrace import FlowRace
from kernel_sdk import ActionContext, Module

mod = Module()
game = FlowRace(mod.settings, mod.data_dir, emit=mod.emit, log=mod.log)
mod.background(game.run_forever)


@mod.status
def status() -> dict:
    return game.snapshot()


@mod.action("server.start")
async def start(ctx: ActionContext) -> dict:
    return await game.start()


@mod.action("server.stop")
async def stop(ctx: ActionContext) -> dict:
    return await game.stop()


@mod.action("server.restart")
async def restart(ctx: ActionContext) -> dict:
    return await game.restart()


@mod.action("game.update")
async def update(ctx: ActionContext, force: bool) -> dict:
    return await game.install(force=force)


@mod.action("share.start")
async def share(ctx: ActionContext, who: str) -> dict:
    return await game.share(who)


@mod.action("share.stop")
async def unshare(ctx: ActionContext) -> dict:
    return await game.unshare()


if __name__ == "__main__":
    mod.run()
