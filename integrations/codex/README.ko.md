# Codex 어댑터

<p align="center">
  <a href="README.md">English</a> · <strong>한국어</strong>
</p>

보통은 최상위 한 줄 설치기를 쓰고, 업그레이드나 나중에 추가한 Codex 설치에는 `calando update`를
씁니다(기본값은 자동 업데이트). 두 경로 모두 어댑터를 원자적으로 병합하고 관련 없는 Hook은 그대로
둡니다. `hooks.json.template`는 수동 확인과 직접 배포용입니다. `__MEMORY_SUPERVISOR_ROOT__`를
슬래시(`/`)를 쓰는 절대 경로로, `__CODEX_HOOKS__`를 해당 Codex 홈의 `hooks.json` 절대 경로로
바꿉니다. 이 소스 경로가 있으면 다른 Codex 홈이 이 사용자 Hook을 프로젝트 Hook으로 다시 발견해도
게이트가 그것을 무시할 수 있습니다.

Codex Hook JSON은 SessionStart, UserPromptSubmit, SubagentStart, SubagentStop, Stop, PreToolUse,
PostToolUse에서 Claude와 호환됩니다. SubagentStop은 모델 컨텍스트를 추가하지 않고 기록만 합니다.
Codex에는 PostToolBatch가 없으므로 PostToolUse가 같은 전환 알림 경로를 호출합니다.

어댑터에는 Codex 0.145.0 이상과 `hooks stable true`가 필요합니다. 네이티브 `PreToolUse`는
`spawn_agent`를 `Agent`로 대응시키므로 일반 `codex`, `codex exec`, IDE 안의 세션이 모두 같은 사전
할당 게이트를 씁니다. 지원하지 않는 릴리스는 수정하지 않습니다. Hook 정의가 바뀐 업데이트 뒤에는
CLI `/hooks` 또는 App **설정 → Hooks**에서 Calando Hook을 다시 신뢰해야 합니다.
[Codex 연결](../../docs/guides/usage-codex.ko.md)을 참고하십시오.
