import asyncio
import json

from gate import Gate, Turns
from vram import Manager, out_of_memory


def turns(**over):
    made = []

    async def before():
        made.append("room")

    t = Turns(over.get("llm_max_wait_s", 5), over.get("image_max_wait_s", 5), before_llm=before, grace_s=0.05)
    t.made = made
    return t


async def settle():
    for _ in range(5):
        await asyncio.sleep(0)


async def test_images_around_an_llm_request_run_together():
    """image 1 running, an LLM request arrives, image 2 arrives: both images, then the LLM."""
    t = turns()
    order = []
    t.set_image_busy(True)  # image 1 is running

    async def chat():
        async with t.llm("Ollama"):
            order.append("llm")

    llm = asyncio.create_task(chat())
    await settle()
    assert order == [] and t.status()["llm_waiting"] == 1

    await asyncio.wait_for(t.image(), 1)  # image 2 goes straight to ComfyUI's queue
    order.append("image 2 queued")
    await settle()
    assert order == ["image 2 queued"]

    t.set_image_busy(False)  # both images done
    await asyncio.sleep(0.1)  # past the submit grace
    t.wake()
    await asyncio.wait_for(llm, 1)
    assert order == ["image 2 queued", "llm"]
    assert t.made == ["room"]  # made room for the LLM first
    assert t.llm_running == 0


async def test_image_jobs_wait_for_a_running_llm():
    t = turns()
    release = asyncio.Event()
    order = []

    async def chat():
        async with t.llm("Ollama"):
            order.append("llm start")
            await release.wait()
            order.append("llm end")

    async def picture():
        await t.image()
        order.append("image")

    llm = asyncio.create_task(chat())
    await settle()
    img = asyncio.create_task(picture())
    await settle()
    assert order == ["llm start"] and t.status()["images_waiting"] == 1
    release.set()
    await asyncio.wait_for(asyncio.gather(llm, img), 1)
    assert order == ["llm start", "llm end", "image"]


async def test_an_llm_never_waits_longer_than_the_limit():
    t = turns(llm_max_wait_s=0.2)
    t.set_image_busy(True)  # images forever
    ran = []

    async def chat():
        async with t.llm("Ollama"):
            ran.append(True)
            await asyncio.sleep(0.2)

    llm = asyncio.create_task(chat())
    await asyncio.sleep(0.35)
    assert ran == [True] and t.overlaps == 1
    # While it runs, new images wait for it.
    img = asyncio.create_task(t.image())
    await settle()
    assert not img.done()
    await asyncio.wait_for(asyncio.gather(llm, img), 1)
    assert t.llm_running == 0


async def test_llm_requests_run_right_away_when_the_gpu_is_free():
    t = turns()
    async with t.llm("Ollama"):
        assert t.status()["turn"] == "llm"
    assert t.made == []  # nothing to make room for


# -- the proxy over real sockets ----------------------------------------------------------


async def fake_upstream():
    """Answers every request with its method and path, streamed in two chunks."""
    seen = []

    async def handle(reader, writer):
        head = await reader.readuntil(b"\r\n\r\n")
        first = head.split(b"\r\n")[0].decode()
        length = next(
            (int(h.split(b":")[1]) for h in head.split(b"\r\n") if h.lower().startswith(b"content-length")), 0
        )
        body = await reader.readexactly(length) if length else b""
        seen.append((first, body, b"connection: close" in head.lower()))
        writer.write(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n")
        writer.write(b'{"line": 1}\n')
        await writer.drain()
        writer.write(json.dumps({"got": first}).encode() + b"\n")
        await writer.drain()
        writer.close()

    server = await asyncio.start_server(handle, "127.0.0.1", 0)
    return server, server.sockets[0].getsockname()[1], seen


async def request(port, method, path, body=b""):
    reader, writer = await asyncio.open_connection("127.0.0.1", port)
    writer.write(
        f"{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: keep-alive\r\n"
        f"Content-Length: {len(body)}\r\n\r\n".encode()
        + body
    )
    await writer.drain()
    data = await reader.read()
    writer.close()
    return data.decode()


def manager_with_proxies(upstream_port):
    settings = {
        "auto": True,
        "min_free_mb": 0,
        "after_s": 15,
        "poll_s": 3.0,
        "llm_max_wait_s": 5,
        "image_max_wait_s": 5,
        "llm_reserve_mb": 0,
        "proxy_host": "127.0.0.1",
        "retry_out_of_memory": True,
        "apps": [
            {"name": "Ollama", "kind": "ollama", "url": f"http://127.0.0.1:{upstream_port}", "proxy_port": 0},
            {"name": "ComfyUI", "kind": "comfyui", "url": f"http://127.0.0.1:{upstream_port}", "priority": 50},
        ],
    }

    class NoGpu:
        def totals(self):
            return None

        def per_process(self):
            return {}

    m = Manager(settings, gpu=NoGpu(), processes=dict)
    m.turns.grace_s = 0.05
    return m


async def start_gate(m, name):
    app = m.app(name)
    gate = Gate(app.name, app.upstream, m._hold_for(app))
    await gate.start("127.0.0.1", 0)
    return gate, gate.server.sockets[0].getsockname()[1]


async def test_proxy_holds_llm_work_but_not_lookups():
    server, port, seen = await fake_upstream()
    m = manager_with_proxies(port)
    _, llm_port = await start_gate(m, "Ollama")
    m.turns.set_image_busy(True)

    tags = await asyncio.wait_for(request(llm_port, "GET", "/api/tags"), 2)
    assert "200 OK" in tags and '"got": "GET /api/tags HTTP/1.1"' in tags

    chat = asyncio.create_task(request(llm_port, "POST", "/api/chat", b'{"model": "qwen3:8b"}'))
    await asyncio.sleep(0.2)
    assert not chat.done() and len(seen) == 1  # held while images run
    m.turns.set_image_busy(False)
    await asyncio.sleep(0.1)
    m.turns.wake()
    reply = await asyncio.wait_for(chat, 2)
    assert '{"line": 1}' in reply and "POST /api/chat" in reply  # streamed through, both chunks
    assert seen[-1][1] == b'{"model": "qwen3:8b"}'
    assert seen[-1][2], "the proxy asks for one request per connection"
    server.close()


async def test_proxy_holds_comfyui_prompts_while_an_llm_runs():
    server, port, seen = await fake_upstream()
    m = manager_with_proxies(port)
    _, comfy_port = await start_gate(m, "ComfyUI")
    release = asyncio.Event()

    async def chat():
        async with m.turns.llm("Ollama"):
            await release.wait()

    llm = asyncio.create_task(chat())
    await settle()
    page = await asyncio.wait_for(request(comfy_port, "GET", "/queue"), 2)
    assert "GET /queue" in page  # the page and status pass straight through
    job = asyncio.create_task(request(comfy_port, "POST", "/prompt", b'{"prompt": {}}'))
    await asyncio.sleep(0.2)
    assert not job.done()
    release.set()
    assert "POST /prompt" in await asyncio.wait_for(job, 2)
    await llm
    server.close()


async def test_proxy_reports_a_stopped_app():
    m = manager_with_proxies(1)  # nothing listens on port 1
    _, llm_port = await start_gate(m, "Ollama")
    reply = await asyncio.wait_for(request(llm_port, "GET", "/api/tags"), 2)
    assert reply.startswith("HTTP/1.1 502") and "Ollama isn't running" in reply


# -- retries --------------------------------------------------------------------------------


def failed(message):
    return {
        "prompt": [7, "p7", {"3": {"class_type": "KSampler", "inputs": {}}}, {"client_id": "ui-123"}, ["9"]],
        "status": {
            "status_str": "error",
            "messages": [
                ["execution_start", {"timestamp": 1}],
                ["execution_error", {"exception_type": "torch.OutOfMemoryError", "exception_message": message}],
            ],
        },
    }


def test_out_of_memory_detection():
    assert out_of_memory(failed("CUDA out of memory. Tried to allocate 2.00 GiB"))
    assert out_of_memory(failed("Allocation on device"))
    not_oom = failed("mat1 and mat2 shapes cannot be multiplied")
    not_oom["status"]["messages"][1][1]["exception_type"] = "RuntimeError"
    assert not out_of_memory(not_oom)
    assert not out_of_memory({"status": {"status_str": "success"}})


async def test_jobs_that_ran_out_of_vram_are_queued_again_once():
    history = {"old": failed("out of memory")}
    posts, events = [], []

    def fetch(method, url, body, timeout):
        path = url.split("/", 3)[-1]
        if path.startswith("history"):
            return history
        if path == "api/ps":
            return {"models": [{"name": "qwen3:8b", "size_vram": 5 << 30}]}
        if path == "system_stats":
            return {"devices": [{"torch_vram_total": 0}]}
        if path == "queue":
            return {"queue_running": [], "queue_pending": []}
        posts.append((path, body))
        return {"prompt_id": "retry-1"} if path == "prompt" else {}

    m = manager_with_proxies(1)
    m.fetch = fetch
    m.emit = lambda kind, message, level="info", **data: events.append(kind)
    await m.refresh()
    await m.retry_failed()
    assert posts == []  # what was there before isn't news

    history["p7"] = failed("CUDA out of memory")
    await m.retry_failed()
    assert posts[0] == ("api/generate", {"model": "qwen3:8b", "keep_alive": 0})  # made room first
    path, body = posts[1]
    assert path == "prompt" and body["client_id"] == "ui-123" and "3" in body["prompt"]
    assert events == ["vram.freed", "job.retried"]

    history["retry-1"] = failed("CUDA out of memory")  # the retry failed too: no third try
    await m.retry_failed()
    assert len(posts) == 2
