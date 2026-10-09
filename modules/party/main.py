"""Party Games: Jackbox-style games hosted on this PC, played on phones, shared through Tailscale."""

from kernel_sdk import ActionContext, Module
from partyroom import Party

mod = Module()
party = Party(mod.settings, mod.data_dir, emit=mod.emit, log=mod.log)
mod.background(party.run_forever)


@mod.status
def status() -> dict:
    return party.snapshot()


@mod.action("server.start")
async def start(ctx: ActionContext) -> dict:
    return await party.start()


@mod.action("server.stop")
async def stop(ctx: ActionContext) -> dict:
    return await party.stop()


@mod.action("room.new")
async def new_room(ctx: ActionContext) -> dict:
    return await party.new_room()


@mod.action("game.end")
async def end_game(ctx: ActionContext) -> dict:
    return await party.end_game()


@mod.action("player.kick")
async def kick(ctx: ActionContext, name: str) -> dict:
    return await party.kick(name)


@mod.action("bot.add")
async def add_bots(ctx: ActionContext, count: int = 1) -> dict:
    return await party.add_bots(count)


@mod.action("bot.remove")
async def remove_bots(ctx: ActionContext) -> dict:
    return await party.remove_bots()


@mod.action("share.start")
async def share(ctx: ActionContext, who: str) -> dict:
    return await party.share(who)


@mod.action("share.stop")
async def unshare(ctx: ActionContext) -> dict:
    return await party.unshare()


@mod.action("content.reload")
async def reload_content(ctx: ActionContext) -> dict:
    return await party.reload_content()


@mod.action("recap.send")
async def recap(ctx: ActionContext) -> dict:
    return await party.send_recap_now()


@mod.action("hits.remove")
async def remove_hit(ctx: ActionContext, id: int) -> dict:
    return await party.remove_hit(id)


@mod.action("player.rename")
async def rename(ctx: ActionContext, name: str, new_name: str) -> dict:
    return await party.rename(name, new_name)


@mod.action("player.set_pin")
async def set_pin(ctx: ActionContext, name: str, pin: str) -> dict:
    return await party.set_pin(name, pin)


@mod.action("player.forget")
async def forget(ctx: ActionContext, name: str) -> dict:
    return await party.forget(name)


if __name__ == "__main__":
    mod.run()
