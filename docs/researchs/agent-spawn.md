# Research: Persistent stdin for Claude CLI (`--input-format stream-json`)

**Date:** 2026-06-30
**Context:** LoopDeck agentic chat — investigating session continuity via persistent process instead of per-turn `--resume <session_id>`

---

## Problem

Current `call_agents()` implementation uses `Command::output()` with `-p` (single-shot prompt mode). Each call spawns a new `claude` process that runs one prompt and exits immediately. Multi-turn conversation requires `--resume <session_id>` on every call, re-spawning the process each time.

**Open question:** what if we never close stdin? Does `claude --input-format stream-json` stay alive and accept multiple turns on the same process, without needing `--resume`?

---

## Finding

Confirmed via manual bash testing: `claude --input-format stream-json --output-format stream-json --verbose` does **not exit** after producing a response when stdin is kept open. It blocks, waiting for the next JSON message on stdin. Closing stdin (EOF) is the signal for it to exit gracefully.

This is a different interaction mode from `-p` (single-shot):

| Mode | Process lifecycle | Multi-turn mechanism |
|---|---|---|
| `-p` (current) | Spawns, runs one prompt, exits | `--resume <session_id>` on next spawn |
| `--input-format stream-json` | Spawns once, stays alive | Keep writing to stdin; same process retains context |

---

## Bash reproduction

### Attempt 1 — named pipe (FIFO): unreliable

```bash
mkfifo /tmp/claude_in
claude --input-format stream-json --output-format stream-json --verbose < /tmp/claude_in &
exec 3> /tmp/claude_in
echo '{"type":"user","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}' >&3
```

Result: `broken pipe` — the backgrounded process exited before the write happened. Likely a race condition between FIFO open and reader attach, or the process treating an initial empty read as EOF. Not used going forward — abandoned in favor of `coproc`.

### Attempt 2 — `coproc` (zsh syntax): working

Note: zsh `coproc` syntax differs from bash. No curly braces, single shared fd named `p` for both read and write (bash uses an array with separate `[0]`/`[1]` fds).

```bash
# Start the coprocess
coproc claude --input-format stream-json --output-format stream-json --verbose 2>/tmp/claude_err.log

# Confirm it's alive and waiting
jobs

# Send a message
echo '{"type":"user","message":{"role":"user","content":[{"type":"text","text":"hi, my name is Suprie"}]}}' >&p

# Read a response line
read -r line <&p
echo "$line"

# Send a follow-up — same process, no --resume needed
echo '{"type":"user","message":{"role":"user","content":[{"type":"text","text":"what is my name?"}]}}' >&p
```

This worked — confirmed the process retains conversation context across multiple stdin writes without `--resume`.

### Shutting it down

```bash
exec p>&-     # close write-end -> sends EOF -> claude exits gracefully
jobs -l       # if needed, get PID
kill %1       # or kill <PID> directly
kill -9 <PID> # last resort
```

---

## Implications for Rust implementation

`Command::output()` is no longer the right primitive — it blocks until process exit and only gives access to stdin/stdout as a single batch. Need to move to `Command::spawn()` with piped stdin/stdout, holding the `Child` (or its stdin/stdout handles) alive across multiple logical "turns."

### Sketch

```rust
use std::process::{Command, Stdio, Child, ChildStdin, ChildStdout};
use std::io::{Write, BufReader};

pub struct ClaudeSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl ClaudeSession {
    pub fn spawn(agent_config: &AgentConfig) -> Result<Self, AppError> {
        let mut cmd = Command::new("claude");
        cmd.args([
            "--input-format", "stream-json",
            "--output-format", "stream-json",
            "--verbose",
        ]);
        // ... set env vars from agent_config (auth_token, base_url, model)

        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped()); // don't inherit — keep stdout clean for parsing

        let mut child = cmd.spawn()
            .map_err(|e| AppError::Agent(format!("failed to spawn claude: {}", e)))?;

        let stdin = child.stdin.take()
            .ok_or_else(|| AppError::Agent("failed to get stdin".into()))?;
        let stdout = child.stdout.take()
            .ok_or_else(|| AppError::Agent("failed to get stdout".into()))?;

        Ok(Self { child, stdin, stdout: BufReader::new(stdout) })
    }

    pub fn send_message(&mut self, text: &str) -> Result<AgentResponse, AppError> {
        let input = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{"type": "text", "text": text}]
            }
        });

        writeln!(self.stdin, "{}", input)
            .map_err(|e| AppError::Agent(format!("write failed: {}", e)))?;
        self.stdin.flush()
            .map_err(|e| AppError::Agent(format!("flush failed: {}", e)))?;

        // Read lines until a "result" event is seen — stdout never hits EOF
        // on its own while the process is alive and idle.
        todo!("loop over self.stdout lines, parse StreamEvent, stop at Result")
    }
}

impl Drop for ClaudeSession {
    fn drop(&mut self) {
        // self.stdin drops here -> EOF sent to child -> claude exits gracefully
        let _ = self.child.wait();
    }
}
```

### Known gotchas to work through

1. **Borrow checker** — writing to `self.stdin` and reading from `self.stdout` within the same `&mut self` method is fine structurally, but interleaving with any shared buffering needs care.
2. **Read termination condition** — must stop reading at a clear event boundary (`StreamEvent::Result`), not at EOF, since stdout will sit open indefinitely between turns.
3. **Partial lines** — `BufReader::read_line` behavior if output is chunked mid-line by the OS pipe needs verification under real load.
4. **State lifecycle in Tauri** — `ClaudeSession` needs to live in `Tauri::State` (e.g. `Mutex<Option<ClaudeSession>>`) to persist across IPC calls from the frontend.
5. **Drop order** — declare `stdin` before `child` if relying on implicit field-drop-order for EOF-then-wait semantics; otherwise close explicitly in `Drop`.

---

## Next steps

- [ ] Implement `send_message` read loop (stop condition: `StreamEvent::Result`)
- [ ] Verify exact input JSON schema expected by `--input-format stream-json` (current schema above is inferred from working bash test — not yet cross-checked against `claude --help` or official docs)
- [ ] Decide: keep this as request/response per `send_message` call, or expose true streaming (emit each `StreamEvent` to frontend as it arrives) — affects whether `call_agents` signature changes to a callback/channel-based API
- [ ] Wire `ClaudeSession` into Tauri state management
- [ ] Compare resource tradeoffs: persistent process per project vs. spawn-per-turn with `--resume` (memory/process count vs. latency)
