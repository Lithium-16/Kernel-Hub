"""Minecraft: start, stop, console, players, whitelist and backups for the server on Pluto."""

from kernel_sdk import ActionContext, Module
from mc import Server, check_player, one_line

mod = Module()
server = Server(mod.settings, log=mod.log)
mod.background(server.poll)
mod.background(server.keepalive)


@mod.status
def status() -> dict:
    return server.status()


@mod.action("server.start")
async def start(ctx: ActionContext) -> dict:
    return await server.start()


@mod.action("server.stop")
async def stop(ctx: ActionContext, delay_min: int) -> dict:
    return await server.stop(delay_min)


@mod.action("server.restart")
async def restart(ctx: ActionContext) -> dict:
    return await server.restart()


@mod.action("server.command")
async def command(ctx: ActionContext, command: str) -> dict:
    return await server.command(command)


@mod.action("server.say")
async def say(ctx: ActionContext, message: str) -> dict:
    return await server.say(message)


@mod.action("console.tail")
async def tail(ctx: ActionContext, lines: int) -> dict:
    return await server.tail(lines)


@mod.action("player.kick")
async def kick(ctx: ActionContext, name: str, reason: str) -> dict:
    reason = one_line(reason, "the reason", 200) if reason.strip() else ""
    return await server.command(f"kick {check_player(name)} {reason}".strip())


@mod.action("player.op")
async def op(ctx: ActionContext, name: str) -> dict:
    return await server.command(f"op {check_player(name)}")


@mod.action("player.deop")
async def deop(ctx: ActionContext, name: str) -> dict:
    return await server.command(f"deop {check_player(name)}")


@mod.action("whitelist.add")
async def whitelist_add(ctx: ActionContext, name: str) -> dict:
    return await server.command(f"whitelist add {check_player(name)}")


@mod.action("whitelist.remove")
async def whitelist_remove(ctx: ActionContext, name: str) -> dict:
    return await server.command(f"whitelist remove {check_player(name)}")


@mod.action("whitelist.list")
async def whitelist_list(ctx: ActionContext) -> dict:
    return await server.whitelist()


@mod.action("world.backup")
async def backup(ctx: ActionContext) -> dict:
    return await server.backup()


@mod.action("mods.list")
async def mods(ctx: ActionContext) -> dict:
    return await server.mods()


@mod.action("service.restart")
async def restart_service(ctx: ActionContext, service: str) -> dict:
    return await server.restart_service(service)


if __name__ == "__main__":
    mod.run()
