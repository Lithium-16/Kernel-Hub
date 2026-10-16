"""PetLink: the relay for the linked desktop pets, shared through Tailscale Funnel."""

from kernel_sdk import ActionContext, Module
from petlink import PetLink

mod = Module()
pets = PetLink(mod.settings, mod.data_dir, emit=mod.emit, log=mod.log)
mod.background(pets.run_forever)


@mod.status
def status() -> dict:
    return pets.snapshot()


@mod.action("user.add")
async def add_user(ctx: ActionContext, name: str, bow_hue: int = 0) -> dict:
    return await pets.add_user(name, bow_hue)


@mod.action("user.remove")
async def remove_user(ctx: ActionContext, name: str) -> dict:
    return await pets.remove_user(name)


@mod.action("share.start")
async def share(ctx: ActionContext, who: str) -> dict:
    return await pets.share(who)


@mod.action("share.stop")
async def unshare(ctx: ActionContext) -> dict:
    return await pets.unshare()


if __name__ == "__main__":
    mod.run()
