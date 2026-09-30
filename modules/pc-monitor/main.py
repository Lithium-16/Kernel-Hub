"""PC monitor: stats, power and Wake-on-LAN for the PC this node runs on."""

from kernel_sdk import ActionContext, Module
from pcmon import Monitor

mod = Module()
monitor = Monitor(mod.settings, emit=mod.emit)
mod.background(monitor.poll)


@mod.status
def status() -> dict:
    return monitor.snapshot


@mod.action("power.sleep")
async def sleep(ctx: ActionContext) -> dict:
    return await monitor.sleep()


@mod.action("power.restart")
def restart(ctx: ActionContext) -> dict:
    return monitor.schedule("restart")


@mod.action("power.shutdown")
def shutdown(ctx: ActionContext) -> dict:
    return monitor.schedule("shutdown")


@mod.action("power.cancel")
def cancel(ctx: ActionContext) -> dict:
    return monitor.cancel()


@mod.action("power.wake")
def wake(ctx: ActionContext, target: str) -> dict:
    return monitor.wake(target)


if __name__ == "__main__":
    mod.run()
