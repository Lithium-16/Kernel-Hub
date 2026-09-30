import asyncio
import importlib.util
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

from comfy import Comfy, Download, load_workflow, normalize_url, prepare, summarize
from kernel_sdk import ActionError

# A default text-to-image workflow, as ComfyUI's "Export (API)" writes it.
WORKFLOW = {
    "3": {
        "class_type": "KSampler",
        "inputs": {
            "seed": 156680208700286,
            "steps": 20,
            "model": ["4", 0],
            "positive": ["6", 0],
            "negative": ["7", 0],
            "latent_image": ["5", 0],
        },
    },
    "4": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": "sdxl.safetensors"}},
    "5": {"class_type": "EmptyLatentImage", "inputs": {"width": 1024, "height": 1024, "batch_size": 1}},
    "6": {"class_type": "CLIPTextEncode", "inputs": {"text": "a cat", "clip": ["4", 1]}},
    "7": {"class_type": "CLIPTextEncode", "inputs": {"text": "blurry", "clip": ["4", 1]}},
    "9": {"class_type": "SaveImage", "inputs": {"images": ["8", 0], "filename_prefix": "ComfyUI"}},
}


def done(pid, ok=True, images=("ComfyUI_00001_.png",)):
    end = ["execution_success", {"prompt_id": pid, "timestamp": 1_000_032_500}]
    if not ok:
        end = ["execution_error", {"node_type": "KSampler", "exception_message": "CUDA out of memory"}]
    return {
        "outputs": {"9": {"images": [{"filename": f, "subfolder": "", "type": "output"} for f in images]}},
        "status": {
            "status_str": "success" if ok else "error",
            "completed": ok,
            "messages": [["execution_start", {"prompt_id": pid, "timestamp": 1_000_000_000}], end],
        },
    }


class FakeComfy:
    """Enough of ComfyUI's HTTP API for the module."""

    def __init__(self):
        self.running, self.pending, self.history, self.posts = [], [], {}, []
        self.next_id = 0
        fake = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def reply(self, body, code=200):
                data = json.dumps(body).encode()
                self.send_response(code)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def do_GET(self):
                if self.path == "/system_stats":
                    self.reply(
                        {
                            "system": {"comfyui_version": "0.3.60"},
                            "devices": [
                                {
                                    "name": "cuda:0 NVIDIA GeForce GTX 1080 Ti : cudaMallocAsync",
                                    "vram_total": 11 * 1024**3,
                                    "vram_free": 8 * 1024**3,
                                }
                            ],
                        }
                    )
                elif self.path == "/queue":
                    self.reply({"queue_running": fake.running, "queue_pending": fake.pending})
                elif self.path.startswith("/history"):
                    self.reply(fake.history)
                elif self.path == "/models/loras":
                    self.reply(["detail.safetensors"])
                else:
                    self.reply({"error": "not found"}, 404)

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])) or b"{}")
                fake.posts.append((self.path, body))
                if self.path == "/prompt":
                    if "999" in body["prompt"]:
                        self.reply(
                            {
                                "error": {"message": "Prompt outputs failed validation"},
                                "node_errors": {
                                    "999": {"class_type": "Nope", "errors": [{"message": "unknown node"}]}
                                },
                            },
                            400,
                        )
                        return
                    fake.next_id += 1
                    pid = f"p{fake.next_id}"
                    fake.pending.append([fake.next_id, pid, body["prompt"], {}, []])
                    self.reply({"prompt_id": pid, "number": fake.next_id, "node_errors": {}})
                else:
                    self.reply({})

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.server.server_port}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def finish(self, pid, **kw):
        self.pending = [j for j in self.pending if j[1] != pid]
        self.running = [j for j in self.running if j[1] != pid]
        self.history[pid] = done(pid, **kw)


def settings(tmp_path, **over):
    base = {
        "url": "http://127.0.0.1:1",
        "comfy_dir": str(tmp_path),
        "start_command": [],
        "models_dir": "",
        "workflows_dir": "",
        "civitai_token": "",
        "hf_token": "",
        "poll_s": 2.0,
    }
    return {**base, **over}


@pytest.fixture
def fake():
    f = FakeComfy()
    yield f
    f.server.shutdown()


@pytest.fixture
def comfy(fake, tmp_path):
    (tmp_path / "kernel-workflows").mkdir()
    (tmp_path / "kernel-workflows" / "SDXL basic.json").write_text(json.dumps(WORKFLOW))
    (tmp_path / "models" / "loras").mkdir(parents=True)
    events = []
    c = Comfy(
        settings(tmp_path, url=fake.url),
        emit=lambda kind, message, level="info", **data: events.append((kind, level, message)),
    )
    c.events = events
    return c


def test_prepare_fills_the_sampler_text_boxes_and_seed():
    g = prepare(WORKFLOW, "a fox", "", 42)
    assert g["6"]["inputs"]["text"] == "a fox"
    assert g["7"]["inputs"]["text"] == "blurry"  # empty negative keeps the workflow's
    assert g["3"]["inputs"]["seed"] == 42
    assert WORKFLOW["6"]["inputs"]["text"] == "a cat"  # the original isn't touched


def test_prepare_prefers_placeholders():
    wf = json.loads(json.dumps(WORKFLOW))
    wf["6"]["inputs"]["text"] = "masterpiece, {{prompt}}, 8k"
    wf["7"]["inputs"]["text"] = "{{negative}}"
    g = prepare(wf, "a fox", "", 1)
    assert g["6"]["inputs"]["text"] == "masterpiece, a fox, 8k"
    assert g["7"]["inputs"]["text"] == ""
    no_text = {"1": {"class_type": "Thing", "inputs": {"seed": 3}}}
    with pytest.raises(ActionError, match="no \\{\\{prompt\\}\\}"):
        prepare(no_text, "a fox", "", 1)
    assert prepare(no_text, "", "", 7)["1"]["inputs"]["seed"] == 7


def test_ui_workflows_are_explained(tmp_path):
    ui = tmp_path / "ui.json"
    ui.write_text(json.dumps({"nodes": [], "links": []}))
    with pytest.raises(ActionError, match="Export \\(API\\)"):
        load_workflow(ui)


def test_summarize_history():
    ok = summarize("p1", done("p1"))
    assert ok == {
        "id": "p1",
        "ok": True,
        "seconds": 32.5,
        "images": ["ComfyUI_00001_.png"],
        "error": None,
        "at": 1_000_032_500,
    }
    bad = summarize("p2", done("p2", ok=False, images=()))
    assert not bad["ok"] and bad["error"] == "KSampler: CUDA out of memory"


def test_download_links():
    assert (
        normalize_url("https://huggingface.co/org/model/blob/main/model.safetensors")
        == "https://huggingface.co/org/model/resolve/main/model.safetensors"
    )
    with pytest.raises(ActionError, match="https"):
        normalize_url("http://example.com/a.safetensors")
    with pytest.raises(ActionError, match="download link"):
        normalize_url("https://civitai.com/models/12345/some-lora")


async def test_status_queue_and_events(comfy, fake):
    fake.history["old"] = done("old")
    s = await comfy.refresh()
    assert s["state"] == "idle"
    assert (s["gpu"], s["vram_used_mb"], s["vram_total_mb"]) == ("NVIDIA GeForce GTX 1080 Ti", 3072, 11264)
    assert comfy.events == []  # history from before isn't news

    r = await comfy.run_workflow("sdxl BASIC", "a fox", "", 10, 2)
    assert [q["seed"] for q in r["queued"]] == [10, 11]
    assert fake.posts[0][1]["prompt"]["6"]["inputs"]["text"] == "a fox"
    fake.running = [fake.pending.pop(0)]
    s = await comfy.refresh()
    assert (s["state"], s["queue_running"], s["queue_pending"], s["current"]) == ("working", 1, 1, "SDXL basic")

    fake.finish("p1")
    fake.running = [fake.pending.pop(0)]
    await comfy.refresh()
    assert comfy.events == [("job.done", "info", "SDXL basic finished in 32.5s (1 image)")]
    fake.finish("p2", ok=False)
    s = await comfy.refresh()
    assert comfy.events[1:] == [
        ("job.failed", "warn", "SDXL basic failed: KSampler: CUDA out of memory"),
        ("queue.finished", "info", "Queue finished: 2 jobs"),
    ]
    assert s["last_job"] == "failed: SDXL basic"
    assert [j["id"] for j in comfy.history(5)["jobs"]] == ["p2", "p1", "old"]


async def test_refused_workflow_says_why(comfy, fake, tmp_path):
    bad = json.loads(json.dumps(WORKFLOW))
    bad["999"] = {"class_type": "Nope", "inputs": {}}
    (tmp_path / "kernel-workflows" / "broken.json").write_text(json.dumps(bad))
    with pytest.raises(ActionError, match="Nope: unknown node"):
        await comfy.run_workflow("broken", "", "", 1, 1)
    with pytest.raises(ActionError, match="Known: broken, SDXL basic"):
        await comfy.run_workflow("missing", "", "", 1, 1)


async def test_queue_controls(comfy, fake):
    fake.pending = [[1, "a", {}, {}, []], [2, "b", {}, {}, []]]
    assert await comfy.clear() == {"removed": 2}
    await comfy.interrupt()
    await comfy.free()
    assert [p for p, _ in fake.posts] == ["/queue", "/interrupt", "/free"]
    assert fake.posts[0][1] == {"clear": True}


async def test_offline_and_back(comfy, fake):
    await comfy.refresh()
    fake.server.shutdown()
    fake.server.server_close()
    s = await comfy.refresh()
    assert s["state"] == "offline"
    assert comfy.events == [("server.stopped", "warn", "ComfyUI stopped")]
    with pytest.raises(ActionError) as e:
        await comfy.interrupt()
    assert e.value.code == "offline"


async def test_models_from_disk_and_api(comfy, tmp_path):
    loras = tmp_path / "models" / "loras"
    (loras / "style").mkdir()
    (loras / "style" / "ink.safetensors").write_bytes(b"x" * 10)
    (loras / "notes.txt").write_text("not a model")
    r = await comfy.list_models("loras")
    assert r == {"folder": "loras", "count": 1, "models": [{"file": "style/ink.safetensors", "bytes": 10}]}
    comfy.models_dir = None
    assert (await comfy.list_models("loras"))["models"] == [{"file": "detail.safetensors"}]
    with pytest.raises(ActionError):
        await comfy.list_models("../etc")


def serve_file(body: bytes, name: str):
    class Handler(BaseHTTPRequestHandler):
        seen_auth = []

        def log_message(self, *args):
            pass

        def do_GET(self):
            Handler.seen_auth.append(self.headers.get("Authorization"))
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Content-Disposition", f'attachment; filename="{name}"')
            self.end_headers()
            self.wfile.write(body)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, Handler


async def test_download_to_a_model_folder(comfy, tmp_path):
    server, handler = serve_file(b"weights" * 1000, "ink.safetensors")
    comfy.settings["civitai_token"] = "secret"
    d = Download(f"http://127.0.0.1:{server.server_port}/file", "loras")
    await comfy._download(d, tmp_path / "models" / "loras")
    assert d.state == "done"
    assert (tmp_path / "models" / "loras" / "ink.safetensors").read_bytes() == b"weights" * 1000
    assert handler.seen_auth == [None]  # tokens only go to their own site
    assert comfy.events[-1][:2] == ("download.done", "info")
    again = Download(d.url, "loras")
    await comfy._download(again, tmp_path / "models" / "loras")
    assert again.state == "failed" and "already in loras" in comfy.events[-1][2]
    server.shutdown()


async def test_download_refuses_bad_names(comfy):
    with pytest.raises(ActionError, match="no model folder"):
        comfy.start_download("https://huggingface.co/a/b/resolve/main/x.safetensors", "nope", "")
    with pytest.raises(ActionError, match="plain file name"):
        comfy.start_download("https://huggingface.co/a/b/resolve/main/x.safetensors", "loras", "..\\evil.safetensors")
    with pytest.raises(ActionError, match="plain file name"):
        comfy.start_download("https://huggingface.co/a/b/resolve/main/x.exe", "loras", "x.exe")


def test_manifest_and_handlers(monkeypatch):
    module_dir = Path(__file__).resolve().parent.parent
    monkeypatch.setenv("KERNEL_MODULE_DIR", str(module_dir))
    spec = importlib.util.spec_from_file_location("comfyui_main", module_dir / "main.py")
    main = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(main)
    main.mod.validate()
    tiers = {a.id: a.ai for a in main.mod.manifest.actions}
    assert tiers["workflow.run"] == "safe"
    assert tiers["queue.clear"] == "confirm" and tiers["models.download"] == "confirm"
    assert asyncio.iscoroutinefunction(main.comfy.poll)
