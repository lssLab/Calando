# 성능과 상주 메모리

<p align="center">
  <a href="performance.md">English</a> · <strong>한국어</strong> · <a href="performance.zh-CN.md">简体中文</a> · <a href="performance.ja.md">日本語</a>
</p>

Memory Supervisor는 동기식 Rust 실행 파일 하나를 사용합니다. 백그라운드 측정이나 hook 판단을
위해 별도 언어 runtime 또는 상주 worker pool을 실행하지 않습니다.

## 네이티브 release build 실측

각 행은 준비 시간을 거친 뒤 0.2초 간격으로 20회 측정한 값입니다. WSL2 행은 실제 설치 서비스,
CI 행은 같은 버전의 플랫폼 실행 파일을 측정했습니다.

| 환경 | 상주 메모리 최소 / 평균 / 최대 | Thread 최소–최대 | Strip 실행 파일 |
| --- | ---: | ---: | ---: |
| WSL2 Linux, 물리 서비스 | 4.88 / **4.88** / 4.88 MiB RSS | 1 | 1.65 MiB |
| Ubuntu x86-64, CI | 3.50 / **3.52** / 3.54 MiB RSS | 1 | 1.69 MiB |
| Windows x86-64, CI | 4.15 / **4.20** / 4.25 MiB working set | 4–6 | 1.34 MiB |
| macOS Apple Silicon, CI | 3.38 / **4.35** / 5.13 MiB RSS | 1–3 | 1.41 MiB |

정상 제어 loop는 single-thread입니다. CI에서 잠깐 늘어나는 thread는 운영체제 센서 명령을 읽는
동안에만 존재하며 제한시간이 있습니다. 모든 실측 최댓값은 인스턴스당 계획값 10 MiB보다
낮았습니다.

## Hook과 상태 조회 지연

| 경로 | 표본 | 결과 |
| --- | ---: | --- |
| WSL2 정상 상태 hook | 200 | 최소 4.29 ms / 평균 4.92 ms / **p95 5.50 ms** / 최대 6.13 ms |
| WSL2 상태 JSON | 50 | 최소 7.37 ms / 평균 8.17 ms / **p95 8.80 ms** / 최대 9.65 ms |

모든 WSL2 p95는 15 ms 이하였습니다.

## 작은 이유

- daemon, hook gate, 상태 조회, 제어, 알림, 연결 관리를 실행 파일 하나가 담당합니다.
- 정상 daemon loop는 동기식이며 Tokio runtime이나 상주 worker pool이 없습니다.
- Linux와 macOS hook은 짧은 정상 상태 lease가 유효하면 느린 검사 경로를 시작하지 않습니다.
- Windows는 비용이 큰 프로세스 목록만 3초 동안 보관하고 전체 메모리 수치는 1초마다 읽습니다.
- 운영체제 센서 명령과 reader thread는 호출 중에만 존재하며 제한시간이 있습니다.

## 수치 해석

RSS와 Windows working set은 운영체제가 집계한 값이므로 고유한 물리 페이지를 byte 단위로 정확히
나타내지는 않습니다. 프로세스 수와 운영체제 센서 구현에 따라 달라질 수 있으므로 용량 계획에는
가장 작은 실측값이 아니라 **설치된 감시 프로그램 하나당 10 MiB**를 사용하세요. Windows, 각 WSL
배포판, VM, 격리 컨테이너는 각각 인스턴스 하나를 실행하므로 상주 메모리도 각각 더해집니다.

정상 상태의 빠른 hook 경로는 daemon이 만든 짧은 최신 판단이 유효할 때만 사용됩니다. 판단이
오래됐거나 경로가 다르면 Rust gate가 로컬 상태와 여러 환경 연동 상태를 다시 확인합니다. 이
검사는 daemon이 멈췄을 때 과거의 정상 판단을 계속 사용하지 않도록 합니다.
