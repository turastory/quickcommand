# Progress Message Diversity Design

## Summary

`quickcommand`가 응답을 생성하는 동안 보여주는 진행 문구를 고정 문자열 하나에서 벗어나, 요청마다 랜덤하게 선택되는 위트 있는 문구로 확장한다. 각 문구에는 현재 실제로 사용 중인 Ollama 모델 이름을 포함한다.

예시:

- `qwen3.5:9b is working hard...`
- `qwen3.5:9b is sharpening a shell command...`
- `qwen3.5:9b is translating intent into terminal magic...`

## Goals

- 진행 스피너 문구를 요청마다 다르게 보여준다.
- 문구 안에 실제 모델명을 넣어 현재 어떤 모델이 동작 중인지 드러낸다.
- 기존 스피너 동작 방식과 사용자 흐름은 유지한다.
- 테스트를 불안정하게 만들지 않는다.

## Non-Goals

- 진행 중에 문구를 주기적으로 바꾸는 회전형 애니메이션은 추가하지 않는다.
- 사용자 설정 파일에서 템플릿을 커스터마이즈하는 기능은 추가하지 않는다.
- Ollama 외 다른 provider 지원은 이번 변경 범위에 포함하지 않는다.

## Current State

현재 진행 문구는 [src/app.rs](/Users/nayoonho/toy/quickcommand/src/app.rs) 에 `PROGRESS_MESSAGE` 상수로 고정되어 있으며, 모든 백엔드 호출에서 동일한 `"Generating response..."` 문자열이 사용된다. 테스트도 이 고정 문자열을 기준으로 작성되어 있다.

## Proposed Design

### 1. Progress message generator

[src/app.rs](/Users/nayoonho/toy/quickcommand/src/app.rs) 에 모델명을 입력받아 진행 문구를 반환하는 작은 헬퍼를 추가한다.

이 헬퍼는:

- 내부에 비어 있지 않은 템플릿 목록을 가진다.
- 템플릿에는 모델명을 삽입할 수 있는 자리표시자가 있다.
- 함수 호출 시 템플릿 하나를 랜덤으로 선택한다.
- 선택된 템플릿에 모델명을 넣은 최종 문자열을 반환한다.

템플릿은 코드 내부 상수로 유지한다. 이번 변경에서는 외부 설정이나 지역화는 도입하지 않는다.

### 2. Integration point

응답 생성을 시작하는 지점에서 `ResolvedConfig.ollama_model` 값을 사용해 진행 문구를 생성하고, 그 결과를 `ui.start_progress(...)` 에 넘긴다.

이 변경은 다음 특성을 가진다.

- 한 요청 안에서는 문구가 고정된다.
- 새 요청이 시작될 때만 새 문구를 다시 선택한다.
- 기존 스피너 프레임(`|`, `/`, `-`, `\`)과 표시 주기는 그대로 유지한다.

### 3. Randomness strategy

랜덤성은 "요청 시작 시 한 번 선택"으로 한정한다. 구현은 표준 라이브러리 또는 현재 의존성과 잘 맞는 가벼운 방법을 사용하되, 호출부가 불필요하게 복잡해지지 않도록 한다.

핵심 요구사항은 다음 두 가지다.

- 선택 결과가 템플릿 목록 중 하나여야 한다.
- 모델명은 항상 최종 문구에 포함되어야 한다.

## Error Handling

- 템플릿 목록은 코드에 내장된 정적 목록으로 유지한다.
- 목록이 비어 있을 가능성은 구현에서 허용하지 않는다.
- 그래도 방어적으로 기본 문구 하나는 항상 보장해 예외 없이 문자열을 반환한다.

즉, 진행 문구 선택 때문에 응답 생성 흐름이 실패해서는 안 된다.

## Testing

기존 테스트는 `"Generating response..."` 정확 일치에 의존하므로, 이번 변경 이후에는 다음처럼 검증한다.

- 진행 이벤트가 `start` 와 `stop` 쌍으로 기록되는지 확인한다.
- `start:` 이벤트 문자열에 현재 설정된 모델명이 포함되는지 확인한다.
- clarification flow 에서도 각 백엔드 호출마다 진행 시작/종료가 반복되는지 확인한다.
- 백엔드 에러가 발생해도 진행 종료 이벤트가 빠지지 않는지 확인한다.

랜덤성 때문에 테스트가 흔들리지 않도록, 테스트는 특정 템플릿 하나를 기대하지 않는다. 필요하면 문구 생성 함수를 분리해 테스트에서 선택 결과를 통제할 수 있게 한다.

## Implementation Notes

- 변경 중심 파일은 [src/app.rs](/Users/nayoonho/toy/quickcommand/src/app.rs) 이다.
- 필요 시 랜덤 선택을 위한 의존성 추가 또는 최소한의 보조 함수 추가가 포함될 수 있다.
- 테스트 수정 범위는 [src/app.rs](/Users/nayoonho/toy/quickcommand/src/app.rs) 의 단위 테스트가 우선이다.

## Acceptance Criteria

- `qc` 실행 시 응답 생성 중 고정 문구 대신 모델명이 들어간 랜덤 진행 문구가 표시된다.
- 한 요청이 진행되는 동안 문구가 중간에 바뀌지 않는다.
- 다음 요청에서는 다른 템플릿이 선택될 수 있다.
- 기존 progress lifecycle 은 유지되며 테스트가 안정적으로 통과한다.
