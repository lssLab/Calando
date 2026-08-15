# Windows 실행 파일 신뢰

<p align="center">
  <a href="windows-signing.md">English</a> · <strong>한국어</strong>
</p>

Windows용 실행 파일은 현재 [SignPath Foundation](https://signpath.org/)의 오픈소스 코드 서명
심사가 진행 중이므로, 완료될 때까지 Windows 11에서는 Smart App Control을 끄고 사용해야 합니다.
현재 준비 버전에는 Authenticode 코드 서명이 없습니다. 설치기는 release와 함께 게시된
SHA-256 checksum을 먼저 검증하지만, 이 무결성 검사는 Windows가 요구하는 게시자 서명을 대신하지
않습니다.

## 어떤 경우에 영향을 받나

- PowerShell, Windows Terminal 또는 Windows 네이티브 Codex App Server에서 실행하면 Windows
  네이티브 경로이므로 Smart App Control의 영향을 받습니다.
- Windows의 Codex App 화면을 사용하더라도 App Server와 도구가 WSL 안에서 실행되면 WSL용
  Supervisor를 사용합니다. Windows 실행 파일을 실행하지 않으므로 이 안내가 적용되지 않습니다.
- 회사의 App Control 정책, Windows 11 S 모드, 다운로드 평판을 확인하는 SmartScreen은 Smart App
  Control과 별개의 제한일 수 있습니다. 설치기는 이런 정책을 우회하지 않습니다.

## 현재 설치 조건

Smart App Control에는 프로그램 하나만 예외로 허용하는 기능이 없습니다. 서명되지 않은 Windows
네이티브 버전을 사용할 때는 **Windows 보안 → 앱 및 브라우저 컨트롤 → Smart App Control**에서
상태를 확인합니다.

| Windows 상태 | 결과 |
| --- | --- |
| Windows 10 64비트 | Smart App Control이 없으므로 별도 설정 없이 설치할 수 있습니다. SmartScreen 안내가 나오면 다운로드 출처가 이 저장소의 release인지 확인합니다. |
| Smart App Control이 이미 `끔` | 네이티브 설치를 진행할 수 있습니다. 별도 SmartScreen 안내가 나오면 다운로드 출처가 이 저장소의 release인지 확인합니다. |
| 다시 켜는 스위치가 제공되는 최신 Windows 11 | 사용 중에는 `끔`으로 바꿔 설치할 수 있고, 사용을 끝낸 뒤 같은 화면에서 다시 켤 수 있습니다. |
| 다시 켜는 스위치가 없는 Windows 11 | 끄면 다시 켜기 위해 Windows 초기화나 재설치가 필요할 수 있으므로 먼저 이 점을 확인합니다. |
| Windows 11 S 모드 또는 회사 정책이 차단하는 환경 | Windows 네이티브 지원 대상이 아닙니다. WSL처럼 별도로 허용된 환경에서 실행하지 않는 한 설치기로 우회할 수 없습니다. |

`Win + R`에서 `winver`를 실행하면 Windows 버전과 build를 확인할 수 있습니다. 다시 켜는 스위치는
Windows 11 24H2 build 26100.8117 이상과 25H2 build 26200.8117 이상에서 단계적으로 제공되므로,
끄기 전에 실제 설정 화면에 스위치가 보이는지 확인하세요. Microsoft의
[Smart App Control FAQ](https://support.microsoft.com/en-US/Windows/Security/Threat-Malware-Protection/smart-app-control-frequently-asked-questions)와
[배포 안내](https://support.microsoft.com/en-au/help/5079391)에서 최신 기준을 확인할 수 있습니다.

## 다운로드 확인

한 줄 설치 명령은 release source와 실행 파일을 내려받고 함께 게시된 SHA-256 값을 자동으로
검증합니다. 수동으로 내려받은 실행 파일의 서명 상태는 PowerShell에서 다음처럼 확인할 수 있습니다.

```powershell
Get-AuthenticodeSignature .\memory-supervisor.exe | Format-List Status, StatusMessage, SignerCertificate
```

현재 준비 버전은 `NotSigned`가 예상 결과입니다. release가 코드 서명된 상태로 바뀌면 설치 안내와
release notes에 명시하고 이 문서의 조건도 함께 갱신합니다.
