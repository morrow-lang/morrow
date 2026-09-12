#!/usr/bin/env python3
"""Check the Rust default's real JSON-RPC lifecycle, navigation and versioned edits."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
DOC1 = "untitled:lsp-rpc-main.fn"
DOC2 = "untitled:lsp-rpc-result.fn"
INCOMPLETE = "fn add(x: Int, y: Int) -> Int:\n    x + y\n\nfn main() -> Int:\n    ad\n"
VALID = INCOMPLETE.replace("    ad\n", "    add(x: 1, y: 2)\n")
DIRTY = 'fn main() -> Int:\n    fs.read("notes.txt")  \n    0\n'
FORMATTED = 'fn main() -> Int:\n    fs.read("notes.txt")\n    0\n'


def envelope(method: str, params: dict, request_id: int | None = None) -> bytes:
    """Encode a notification or request with byte-counted UTF-8 framing."""
    payload = {"jsonrpc": "2.0", "method": method, "params": params}
    if request_id is not None:
        payload["id"] = request_id
    body = json.dumps(payload, separators=(",", ":"), ensure_ascii=False).encode()
    return f"Content-Length: {len(body)}\r\n\r\n".encode("ascii") + body


def parse_lsp_output(raw: bytes) -> list[dict]:
    """Require complete JSON-RPC frames with no truncated or unframed output."""
    messages = []
    offset = 0
    while offset < len(raw):
        header_end = raw.find(b"\r\n\r\n", offset)
        assert header_end >= 0, "invalid LSP output: unfinished header"
        lengths = [line.split(":", 1)[1].strip()
                   for line in raw[offset:header_end].decode("ascii").split("\r\n")
                   if line.lower().startswith("content-length:")]
        assert len(lengths) == 1 and lengths[0].isdigit(), "invalid Content-Length"
        offset = header_end + 4
        length = int(lengths[0])
        body = raw[offset:offset + length]
        assert len(body) == length, "invalid LSP output: truncated frame"
        offset += length
        message = json.loads(body.decode("utf-8"))
        assert message.get("jsonrpc") == "2.0", message
        messages.append(message)
    return messages


def response(messages: list[dict], request_id: int) -> dict:
    """Require one response for each request, preserving errors for explicit assertions."""
    found = [m for m in messages if m.get("id") == request_id]
    assert len(found) == 1, (request_id, found)
    assert ("result" in found[0]) != ("error" in found[0]), found[0]
    return found[0]


def opened(uri: str, text: str) -> dict:
    """Create a version-one open notification from an unsaved source buffer."""
    return {"textDocument": {"uri": uri, "languageId": "fern", "version": 1, "text": text}}


def flow() -> bytes:
    """Exercise incomplete completion, checked rename, real source action and shutdown."""
    position = {"textDocument": {"uri": DOC1}, "position": {"line": 4, "character": 5}}
    action = {"textDocument": {"uri": DOC2},
              "range": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 8}},
              "context": {"diagnostics": []}}
    return b"".join([
        envelope("initialize", {"capabilities": {
            "workspace": {"workspaceEdit": {"documentChanges": True}},
            "textDocument": {"rename": {"prepareSupport": True}, "codeAction": {
                "codeActionLiteralSupport": {"codeActionKind": {"valueSet": ["source.fixAll"]}}}},
        }}, 1),
        envelope("initialized", {}),
        envelope("textDocument/didOpen", opened(DOC1, INCOMPLETE)),
        envelope("textDocument/completion", {**position, "position": {"line": 4, "character": 6}}, 2),
        envelope("textDocument/rename", {**position, "newName": "sum"}, 3),
        envelope("textDocument/didChange", {"textDocument": {"uri": DOC1, "version": 2},
                                             "contentChanges": [{"text": VALID}]}),
        envelope("textDocument/prepareRename", position, 4),
        envelope("textDocument/rename", {**position, "newName": "sum"}, 5),
        envelope("textDocument/didOpen", opened(DOC2, DIRTY)),
        envelope("textDocument/codeAction", action, 6),
        envelope("textDocument/codeAction", {**action, "context": {"diagnostics": [], "only": ["quickfix"]}}, 7),
        envelope("shutdown", {}, 8),
        envelope("exit", {}),
    ])


def apply_edit(source: str, workspace: dict, uri: str, version: int) -> str:
    """Apply only the expected document/version edits using protocol UTF-16 positions."""
    changes = workspace["documentChanges"]
    assert len(changes) == 1, changes
    assert changes[0]["textDocument"] == {"uri": uri, "version": version}, changes
    lines = source.splitlines(keepends=True)

    def offset(position: dict) -> int:
        """Translate a valid UTF-16 source position without splitting surrogate pairs."""
        line, character = position["line"], position["character"]
        prefix = "".join(lines[:line])
        if line == len(lines):
            assert character == 0
            return len(prefix)
        return len(prefix) + len(lines[line].encode("utf-16-le")[:character * 2].decode("utf-16-le"))

    edits = [(offset(e["range"]["start"]), offset(e["range"]["end"]), e["newText"])
             for e in changes[0]["edits"]]
    assert edits == sorted(edits), edits
    assert all(left[1] <= right[0] for left, right in zip(edits, edits[1:])), edits
    for start, end, text in reversed(edits):
        source = source[:start] + text + source[end:]
    return source


def validate(messages: list[dict]) -> None:
    """Assert semantic results, exact edited text, diagnostics and the lifecycle response."""
    caps = response(messages, 1)["result"]["capabilities"]
    assert "completionProvider" in caps, caps
    assert caps["renameProvider"]["prepareProvider"] is True, caps
    assert "source.fixAll.fern" in caps["codeActionProvider"]["codeActionKinds"], caps
    completion = response(messages, 2)["result"]
    assert "add" in [item.get("label") for item in completion["items"]], completion
    assert response(messages, 3)["error"]["code"] == -32803
    prepared = response(messages, 4)["result"]
    assert prepared == {"placeholder": "add", "range": {
        "start": {"line": 4, "character": 4}, "end": {"line": 4, "character": 7}}}, prepared
    rename = response(messages, 5)["result"]
    assert len(rename["documentChanges"][0]["edits"]) == 2, rename
    assert apply_edit(VALID, rename, DOC1, 2) == VALID.replace("add", "sum"), rename
    actions = response(messages, 6)["result"]
    assert len(actions) == 1 and actions[0]["kind"] == "source.fixAll.fern", actions
    assert "command" not in actions[0], actions
    assert apply_edit(DIRTY, actions[0]["edit"], DOC2, 1) == FORMATTED, actions
    assert response(messages, 7)["result"] == []
    diagnostics = [m["params"]["diagnostics"] for m in messages
                   if m.get("method") == "textDocument/publishDiagnostics"
                   and m["params"]["uri"] == DOC2]
    assert diagnostics and any("Result" in d["message"] for d in diagnostics[-1]), diagnostics
    assert response(messages, 8)["result"] is None


def main() -> int:
    """Run the default compiler, or an explicit fresh compiler, without native backends."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "bin/fern")
    args = parser.parse_args()
    result = subprocess.run([str(args.compiler.resolve()), "lsp"], input=flow(),
                            capture_output=True, timeout=15)
    assert result.returncode == 0, (result.returncode, result.stderr.decode(errors="replace"))
    assert not result.stderr, result.stderr.decode(errors="replace")
    validate(parse_lsp_output(result.stdout))
    print("LSP RPC smoke checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
