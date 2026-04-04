# Progress Message Diversity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 응답 생성 중 표시되는 진행 문구를 요청마다 랜덤하게 바꾸고, 각 문구에 실제 Ollama 모델명을 포함한다.

**Architecture:** 진행 문구 생성 책임을 `src/app.rs` 내부의 작은 헬퍼 함수로 모은다. `run_task_with_deps` 에서 백엔드 호출 직전에 현재 `ResolvedConfig.ollama_model` 값을 이용해 문구를 한 번 생성하고, 그 문자열을 `generate_with_progress` 에 전달해 한 요청 동안 고정된 문구를 사용한다.

**Tech Stack:** Rust 2024, CLI app, existing unit tests in `src/app.rs`, `cargo test`

---

## File Structure

- Modify: `src/app.rs`
  - 진행 문구 템플릿 상수, 문구 선택 헬퍼, `generate_with_progress` 시그니처, 진행 이벤트 테스트를 함께 관리한다.
- Modify: `Cargo.toml`
  - 랜덤 선택을 위해 `rand` 의존성을 추가한다.
- Create: `docs/superpowers/plans/2026-04-04-progress-message-diversity.md`
  - 이 구현 계획 문서다.

### Task 1: Lock In Progress Message Behavior With Tests

**Files:**
- Modify: `src/app.rs`
- Test: `src/app.rs`

- [ ] **Step 1: Rewrite the fixed-string progress assertions into model-aware assertions**

Update the three progress-related tests in `src/app.rs` so they no longer expect `"Generating response..."` exactly. Replace the assertion blocks with the following shape:

```rust
        assert_eq!(ui.progress_events.len(), 2);
        assert!(ui.progress_events[0].starts_with("start:"));
        assert!(ui.progress_events[0].contains("qwen3.5:9b"));
        assert_eq!(ui.progress_events[1], "stop");
```

For the clarification-flow test, use this exact assertion block:

```rust
        assert_eq!(ui.progress_events.len(), 4);
        assert!(ui.progress_events[0].starts_with("start:"));
        assert!(ui.progress_events[0].contains("qwen3.5:9b"));
        assert_eq!(ui.progress_events[1], "stop");
        assert!(ui.progress_events[2].starts_with("start:"));
        assert!(ui.progress_events[2].contains("qwen3.5:9b"));
        assert_eq!(ui.progress_events[3], "stop");
```

- [ ] **Step 2: Run the focused progress tests and verify they fail**

Run:

```bash
cargo test command_generation_wraps_backend_call_with_progress
cargo test clarification_flow_starts_progress_for_each_backend_call
cargo test backend_error_still_stops_progress
```

Expected: FAIL because the production code still emits `Generating response...` and does not guarantee the model name appears in `start:` events.

- [ ] **Step 3: Add a unit test for the new helper that enforces model-name injection**

Add this test near the other `#[cfg(test)]` tests in `src/app.rs`:

```rust
    #[test]
    fn progress_message_includes_model_name() {
        let message = progress_message_for_model("qwen3.5:9b");

        assert!(message.contains("qwen3.5:9b"));
        assert_ne!(message.trim(), "qwen3.5:9b");
    }
```

- [ ] **Step 4: Run the helper test and verify it fails**

Run:

```bash
cargo test progress_message_includes_model_name
```

Expected: FAIL with an error similar to `cannot find function 'progress_message_for_model' in this scope`.

- [ ] **Step 5: Commit the failing-test checkpoint**

```bash
git add src/app.rs
git commit -m "test: cover dynamic progress messages"
```

### Task 2: Implement Randomized Model-Aware Progress Messages

**Files:**
- Modify: `src/app.rs`
- Modify: `Cargo.toml`
- Test: `src/app.rs`

- [ ] **Step 1: Add the randomness dependency**

Add this line under `[dependencies]` in `Cargo.toml`:

```toml
rand = "0.8.5"
```

- [ ] **Step 2: Add the progress templates and helper in `src/app.rs`**

Near the existing progress constants at the top of `src/app.rs`, replace the fixed-message constant with a template list and helper:

```rust
use rand::seq::SliceRandom;

const PROGRESS_TEMPLATES: &[&str] = &[
    "{} is working hard...",
    "{} is sharpening a shell command...",
    "{} is translating intent into terminal magic...",
    "{} is lining up the next terminal move...",
    "{} is rummaging through shell lore...",
];
const DEFAULT_PROGRESS_TEMPLATE: &str = "{} is working hard...";
const PROGRESS_DELAY: Duration = Duration::from_millis(120);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(80);

fn progress_message_for_model(model: &str) -> String {
    let template = PROGRESS_TEMPLATES
        .choose(&mut rand::thread_rng())
        .copied()
        .unwrap_or(DEFAULT_PROGRESS_TEMPLATE);

    template.replacen("{}", model, 1)
}
```

- [ ] **Step 3: Thread the generated message through the request lifecycle**

Update `run_task_with_deps` and `generate_with_progress` in `src/app.rs` to build the message once per backend call and pass it into the progress UI:

```rust
        let progress_message = progress_message_for_model(&config.ollama_model);

        match generate_with_progress(backend, ui, &request, context, &progress_message)? {
```

```rust
fn generate_with_progress<B: Backend, U: Ui>(
    backend: &B,
    ui: &mut U,
    request: &GenerationRequest,
    context: &RuntimeContext,
    progress_message: &str,
) -> Result<ModelReply> {
    ui.start_progress(progress_message)?;
    let generate_result = backend.generate(request, context);
    let stop_result = ui.stop_progress();

    match (generate_result, stop_result) {
        (Ok(reply), Ok(())) => Ok(reply),
        (Ok(_), Err(stop_err)) => Err(stop_err),
        (Err(generate_err), Ok(())) => Err(generate_err),
        (Err(generate_err), Err(_stop_err)) => Err(generate_err),
    }
}
```

- [ ] **Step 4: Run the focused tests and confirm they pass**

Run:

```bash
cargo test progress_message_includes_model_name
cargo test command_generation_wraps_backend_call_with_progress
cargo test clarification_flow_starts_progress_for_each_backend_call
cargo test backend_error_still_stops_progress
```

Expected: PASS. The progress tests should now only care that the start events include `qwen3.5:9b`, and the helper test should confirm model-name injection.

- [ ] **Step 5: Run the full test suite**

Run:

```bash
cargo test
```

Expected: PASS for the full suite with no regressions in copy, emit, init, config, or backend behavior.

- [ ] **Step 6: Commit the implementation**

```bash
git add Cargo.toml src/app.rs
git commit -m "feat: randomize progress messages per model"
```
