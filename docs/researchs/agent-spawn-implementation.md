# Implementation: Persistent stdin for Claude CLI

**Date:** 2026-07-02
**Companion to:** [`agent-spawn.md`](./agent-spawn.md) (the research that motivated this)
**Commits:** `4a10df9` (config foundation), `192ac8a` (ClaudeSession) on `feat/claude-session`

---

## TL;DR

We turned the research from `agent-spawn.md` into working code: a `ClaudeSession`
struct that spawns `claude --input-format stream-json` once and keeps stdin open,
so multi-turn conversation context lives **inside the process** — no per-turn
`--resume`. The "batch" variant returns one `AgentResponse` per `send_message`
call. Two ignored integration tests validate the approach against a live
provider; the cross-turn **context-retention test passes**, confirming the
research hypothesis.

This doc explains *every* decision so you can learn from it and extend it
(streaming, the Tauri state wiring, etc.).

---

## What changed, and the order to read it in

Three files changed for the session work (the config-foundation commit is
separate; see [Commit layout](#commit-layout) at the end):

1. **`src-tauri/src/agents.rs`** — refactor: extract the parser into a reusable
   `ResponseAccumulator` so the live read loop and the old batch parser share
   one implementation.
2. **`src-tauri/src/claude_session.rs`** *(new)* — the `ClaudeSession` struct,
   `send_message` read loop, the fixed `Drop`, and the tests.
3. **`src-tauri/src/lib.rs`** — one line: `mod claude_session;`.

Read this doc top to bottom; it mirrors how the code fits together.

---

## The core problem the refactor solves

Before this work, `agents.rs` had one function, `parse_response(ndjson: &str)`,
that did **two jobs at once**:

1. Walk every line of an NDJSON blob.
2. Accumulate the fields (text, thinking, result, usage…) into an `AgentResponse`.

That's fine when you have the whole blob up front (the old `call_agents` path,
which spawns a process, waits for it to exit, and reads all of stdout). But
`ClaudeSession::send_message` has a different shape: it reads stdout **line by
line as it arrives** and must stop the moment it sees the terminal `result`
event. It can't wait for EOF — the process is deliberately kept alive.

If we left `parse_response` as-is, `send_message` would have to **duplicate**
the accumulation logic. Two copies of "how do I turn stream events into a
response?" would drift apart over time. So we split the two jobs:

| Job | Lives in |
|---|---|
| "How do I turn events into a response?" | `ResponseAccumulator` (shared) |
| "Walk the whole blob at once" | `parse_response` (batch driver) |
| "Read lines until the result event" | `send_message` (streaming driver) |

One accumulation implementation, two drivers. This is the single most important
idea in the change.

---

## `agents.rs` — the reusable parser

### Make `StreamEvent` visible to the sibling module

```rust
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum StreamEvent {        // was: enum StreamEvent
```

`pub(crate)` = "visible anywhere inside this crate, invisible outside the
binary." We need this because `claude_session` calls `parse_stream_line(...)`,
which **returns** a `StreamEvent`. In Rust, if a function returns a private
type, callers can't name it in a `match` — the compiler errors with
`type StreamEvent is private`. `pub(crate)` is the minimum visibility that
unblocks the sibling module without leaking internals to the outside world.

> **Lesson:** "Expose the minimum that unblocks the caller." `pub` would also
> compile, but it would leak an internal parser type into the crate's public
> API. `pub(crate)` is the right knob.

### `ResponseAccumulator` — the shared heart

```rust
#[derive(Default)]
pub(crate) struct ResponseAccumulator {
    text: String,
    thinking: Option<String>,
    result_text: String,
    is_error: bool,
    duration_ms: u64,
    usage: Option<UsageInfo>,
    session_id: String,
}
```

These are exactly the `let mut` locals `parse_response` used to declare inline.
We just hoisted them into a struct. `#[derive(Default)]` gives every field its
default (`String::new()`, `false`, `None`, `0`) for free — that's why `new()`
just delegates to `default()`.

The key method is `ingest_event`, and its signature is the important design
decision:

```rust
#[must_use]
pub(crate) fn ingest_event(&mut self, event: StreamEvent) -> bool {
```

It does two things:
1. **Mutates** the accumulator with whatever the event carries.
2. **Returns `bool`** — `true` if this was the terminal `result` event.

That `bool` is the *protocol* between the accumulator and the read loop. The
batch driver ignores it; the streaming driver uses it as its loop-exit
condition. By having `ingest_event` return it, "what counts as a turn
boundary?" is defined in **one place** — the accumulator — instead of each
driver re-deriving it by re-matching on the event type.

`#[must_use]` makes the compiler warn if a caller drops the bool on the floor.
We hit that warning ourselves in `parse_response` (which intentionally drains
the whole blob) and silenced it with `let _ =`.

The body is the old `match event { ... }` arms, just `self.`-prefixed, so we
won't reproduce it here — read it in `agents.rs`.

### `finish` consumes the accumulator

```rust
pub(crate) fn finish(self) -> Option<AgentResponse> {
    if self.result_text.is_empty() && self.text.is_empty() {
        return None;
    }
    Some(AgentResponse { /* fields moved out of self */ })
}
```

`self` **by value** (not `&mut self`) means this *consumes* the accumulator —
you can't accidentally keep feeding it after building the response. The
"return `None` if nothing meaningful came back" guard preserves the old
behavior so `test_parse_response_empty_input` still passes.

### `parse_stream_line` — tolerant line parsing

```rust
pub(crate) fn parse_stream_line(line: &str) -> Option<StreamEvent> {
    serde_json::from_str::<StreamEvent>(line.trim()).ok()
}
```

- `.ok()` turns `Result` into `Option` — parse failures become `None`.
  Callers decide what to do: both drivers **skip** unknown lines silently,
  so a stray blank line or stderr leak can't kill a turn.
- `.trim()` defensively strips whitespace/newlines before parsing.

### `parse_response` is now a thin wrapper

```rust
fn parse_response(ndjson: &str) -> Option<AgentResponse> {
    let mut acc = ResponseAccumulator::new();
    for line in ndjson.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(event) = parse_stream_line(line) {
            let _ = acc.ingest_event(event);   // ignore is_result — we drain everything
        }
    }
    acc.finish()
}
```

Same behavior as before (the existing tests confirm it), but ~6 lines instead
of ~40, because the work moved into the accumulator. The `let _ =` is the
explicit "yes, I'm intentionally ignoring the must-use return here" — correct,
because batch parsing reads the whole blob regardless of where the result sits.

### `apply_agent_config` is now `pub(crate)`

So `ClaudeSession::spawn` can call `crate::agents::apply_agent_config(...)`
instead of carrying its own copy. One implementation of "turn an `AgentConfig`
into env vars" shared by the single-shot (`call_agents`) and persistent
(`ClaudeSession`) paths.

---

## `claude_session.rs` — the session itself

### The struct: `stdin` is an `Option`

```rust
pub struct ClaudeSession {
    child: Child,
    stdin: Option<ChildStdin>,        // was: ChildStdin in the research sketch
    stdout: BufReader<ChildStdout>,
}
```

**This is a real bug fix over the research sketch.** The sketch's `Drop` was:

```rust
fn drop(&mut self) {
    let _ = self.child.wait();   // ← DANGER: deadlock
}
```

It relied on "stdin drops → EOF → claude exits → `wait()` returns." The problem:
`drop()` runs **while the struct is still alive**. `self.child.wait()` blocks
waiting for claude to exit, but claude won't exit until it sees EOF on stdin,
and it won't see EOF until `self.stdin` drops — which happens **after `drop()`
returns**. Circular wait = **deadlock**. If a `ClaudeSession` ever got dropped
while claude was mid-turn, the whole app would hang.

Making `stdin` an `Option` lets us **explicitly** close it at the top of `Drop`
via `self.stdin.take()` (which empties the Option and drops the inner
`ChildStdin`, sending EOF). That breaks the cycle. It also doubles as a
"still usable" flag — `send_message` errors if stdin is already `None`.

> **Lesson:** Field-drop order in Rust *can* be relied on (fields drop in
> declaration order), but it's subtle and brittle to refactor. When shutdown
> ordering is load-bearing, make it **explicit** in `Drop` instead. Future-you
> won't have to remember the declaration order matters.

### `send_message` — the read loop

The write half writes one user-turn JSON line to stdin and flushes:

```rust
let stdin = self.stdin.as_mut()
    .ok_or_else(|| AppError::Agent("session stdin already closed".into()))?;

let input = serde_json::json!({
    "type": "user",
    "message": {
        "role": "user",
        "content": [{"type": "text", "text": text}]
    }
});
writeln!(stdin, "{}", input)?;
stdin.flush()?;
```

- `.as_mut()` on `&mut Option<T>` gives `Option<&mut T>` — borrow the inner
  stdin mutably, erroring if it's already been taken.
- `writeln!` adds the trailing `\n` that NDJSON requires.
- `flush` forces the bytes out of any OS buffer immediately; without it,
  claude might sit waiting for data that's stuck in our buffer.

Then the loop — the part that was `todo!()` in the research sketch:

```rust
let mut acc = ResponseAccumulator::new();
let mut line = String::new();
loop {
    line.clear();
    let n = self.stdout.read_line(&mut line)
        .map_err(|e| AppError::Agent(format!("read failed: {}", e)))?;
    if n == 0 {
        return Err(AppError::Agent(
            "claude closed stdout before sending a result event".into(),
        ));
    }

    let trimmed = line.trim();
    if trimmed.is_empty() { continue; }

    let is_result = match parse_stream_line(trimmed) {
        Some(event) => acc.ingest_event(event),
        None => continue,
    };
    if is_result { break; }
}
acc.finish().ok_or_else(|| AppError::Agent("no result event from claude".into()))
```

Things to understand, in order of importance:

1. **`line` is allocated once outside the loop; `clear()` each iteration.**
   `clear()` empties the string but keeps its heap allocation, so over many
   turns we reuse the same buffer instead of allocating/freeing per line.
   Standard Rust pattern for hot read loops.
2. **`read_line` *appends* (not writes) to `line`** — which is exactly why we
   `clear()` first. It returns bytes read; **`0` means EOF**.
3. **EOF between turns is impossible for a healthy persistent process** — it
   sits idle with the pipe open. So a `0` here means claude **died**; we
   surface it as an error rather than looping forever or treating it as a
   clean turn end. This is gotcha #2 from the research doc made concrete:
   *stop at the `result` boundary, never at EOF*.
4. **Stop condition is `is_result`, not EOF.** We break the moment the
   accumulator tells us it just ingested a `Result` event. Unknown lines are
   skipped (`None => continue`), so noise can't break the loop.
5. **`finish()` after the loop** builds the response; if claude somehow emitted
   a result event with no text *and* no assistant text, it returns `None` and
   we turn that into an error.

> **Gotcha — the import.** `read_line` is a method on the `BufRead` trait, not
> on `BufReader` directly. To call it you must `use std::io::BufRead;`
> (importing the trait). Forgetting this is a common compile error. The
> original research sketch only imported `BufReader` and `Write` — `read_line`
> wouldn't have compiled as written.

### `Drop` — fixed shutdown

```rust
impl Drop for ClaudeSession {
    fn drop(&mut self) {
        // 1. Close stdin FIRST so claude sees EOF and exits on its own.
        self.stdin.take();

        // 2. Bounded wait, then force-kill so a hung child can't wedge the app.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,                 // reaped cleanly
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                _ => {
                    let _ = self.child.kill();          // SIGKILL
                    let _ = self.child.wait();          // reap the zombie
                    return;
                }
            }
        }
    }
}
```

Step 1 (`stdin.take()`) breaks the deadlock described above. Step 2 deserves
explanation:

- **Why `try_wait()` instead of `wait()`?** A plain `wait()` blocks until the
  child exits. Fine in the normal EOF case, but if claude is wedged (mid
  tool-use, hung on a network call), `wait()` blocks **forever** and the host
  app hangs on shutdown. `try_wait()` is non-blocking: it returns immediately
  with `Ok(Some(status))` if the child has exited, `Ok(None)` if still running.
  Polling it every 50 ms up to a 5-second budget caps the worst case.
- **The `_ =>` arm** catches both "deadline expired" (`Ok(None)` past deadline)
  and "`try_wait` errored" (`Err`). Either way we escalate: `kill()` sends
  SIGKILL, then a final `wait()` reaps the zombie. On Unix you **must** reap
  killed children or they linger as zombies. `let _ =` ignores failures because
  we're in `Drop` — there's nowhere meaningful to report an error.

---

## Tests — proving the design works

Both tests are `#[ignore]`d (they hit a real provider). Run explicitly:

```bash
cargo test --lib claude_session -- --ignored --nocapture
```

`--nocapture` matters: each turn can take seconds, and you want panic output
live, not buffered until the end.

### `test_session_single_turn` — smoke test

Spawns a session, sends one message, asserts a structured reply comes back.
Validates the full pipeline: spawn → write stdin → read loop → stop at result
→ parse. If this fails, the problem is in the plumbing.

### `test_session_retains_context_across_turns` — the thesis test

```rust
// Turn 1 — plant a distinctive fact
let first = session.send_message(
    "...The codeword is: ZUCCHINI. Reply with exactly: understood."
).expect(...);

// Turn 2 — recall it on the SAME process, no --resume
let second = session.send_message(
    "What is the secret codeword I just told you?..."
).expect(...);

assert!(second.result.to_lowercase().contains("zucchini"), ...);
```

**This is the test that proves `agent-spawn.md`'s hypothesis.** The whole
reason `ClaudeSession` exists is context retention across turns on one process.
A model can only answer "zucchini" to turn 2 if it remembers turn 1 — there's
no way to guess it from the second question alone. **This test passes**,
confirming:

- `claude --input-format stream-json` stays alive with stdin open ✅
- Context survives across turns **without** `--resume` ✅
- The inferred input JSON schema (`{"type":"user",...}`) is correct ✅
- The read loop correctly stops at the result event ✅
- `Drop` reaps cleanly (no hang) ✅

> **Lesson:** When a piece of code exists to validate a hypothesis, write the
> test that *would fail* if the hypothesis were false. A single-turn test
> alone couldn't distinguish "persistent process works" from "I could've just
> used `--resume`." The codeword test can.

---

## Things this implementation does NOT do (yet)

So you know what's deliberately out of scope:

1. **No Tauri state wiring.** `ClaudeSession` isn't held anywhere yet. Next
   step: add `session: Mutex<Option<ClaudeSession>>` to `AppState` and expose
   commands (`agent_send_message`, `agent_reset_session`, maybe
   `agent_session_alive`).
2. **No streaming.** `send_message` buffers the whole turn and returns one
   `AgentResponse`. For token-by-token UI you'd emit each `StreamEvent` to the
   frontend via Tauri events instead of accumulating — the accumulator
   refactor above is compatible with that future change.
3. **stderr is piped but never read.** Over many turns an unread stderr pipe
   can fill its ~64 KB kernel buffer and **block claude**, deadlocking the
   session. Either drain stderr on a thread or set it to `Stdio::null()`.
   Fix this before shipping the UI.
4. **Config changes don't affect a live session.** Env vars (auth token,
   model, …) are baked in at `spawn()` time. The escape hatch is
   `agent_reset_session()` — drop the session so the next call re-spawns with
   fresh config.

---

## Commit layout

The work landed as two commits on `feat/claude-session`:

| Commit | Concern | Files |
|---|---|---|
| `4a10df9` | Agent config foundation: relocate `AgentConfig` into `config.rs`, get/set commands, Settings UI | `config.rs`, `commands.rs`, `lib.rs`, frontend files |
| `192ac8a` | `ClaudeSession` + parser refactor + tests | `agents.rs`, `claude_session.rs`, `lib.rs` |

They're separate because they're separate concerns, but note `agents.rs` and
`lib.rs` each contained **both** concerns intermingled (the config relocation
and the parser refactor live in the same file). Splitting them into two
commits required reconstructing a foundation-only intermediate state of those
files rather than a simple `git add -p`. Both commits compile independently —
verified via a throwaway git worktree at `HEAD~1`.

---

## Glossary

- **NDJSON** — newline-delimited JSON: one JSON object per line. What
  `--output-format stream-json` produces, and what `--input-format stream-json`
  expects on stdin.
- **Stream event** — one line of that NDJSON stream: a `system`, `assistant`,
  or `result` JSON object (see `StreamEvent` in `agents.rs`).
- **Turn** — one user message → one assistant response, terminated by a
  `result` event. A persistent process serves many turns.
- **Reaping** — calling `wait()` on a finished child process so the OS can
  free its process slot. On Unix, unreaped children become zombies.
