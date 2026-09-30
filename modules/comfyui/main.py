"""ComfyUI: the queue, saved workflows, models and downloads, through ComfyUI's own API."""

from comfy import Comfy
from kernel_sdk import ActionContext, Module

mod = Module()
comfy = Comfy(mod.settings, emit=mod.emit, log=mod.log)
mod.background(comfy.poll)


@mod.status
def status() -> dict:
    return comfy.snapshot


@mod.action("workflow.run")
async def run(ctx: ActionContext, workflow: str, prompt: str, negative: str, seed: int, count: int) -> dict:
    return await comfy.run_workflow(workflow, prompt, negative, seed, count)


@mod.action("job.interrupt")
async def interrupt(ctx: ActionContext) -> dict:
    return await comfy.interrupt()


@mod.action("queue.clear")
async def clear(ctx: ActionContext) -> dict:
    return await comfy.clear()


@mod.action("memory.free")
async def free(ctx: ActionContext) -> dict:
    return await comfy.free()


@mod.action("workflows.list")
def workflows(ctx: ActionContext) -> dict:
    return comfy.list_workflows()


@mod.action("models.list")
async def models(ctx: ActionContext, folder: str) -> dict:
    return await comfy.list_models(folder)


@mod.action("models.download")
def download(ctx: ActionContext, url: str, folder: str, filename: str) -> dict:
    return comfy.start_download(url, folder, filename)


@mod.action("history.recent")
def history(ctx: ActionContext, limit: int) -> dict:
    return comfy.history(limit)


@mod.action("server.start")
async def start(ctx: ActionContext) -> dict:
    return await comfy.start()


@mod.action("server.stop")
async def stop(ctx: ActionContext) -> dict:
    return await comfy.stop()


if __name__ == "__main__":
    mod.run()
