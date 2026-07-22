# 검증 기록

## 1.0.2 I-Beam 전용 처리 수정

다음 항목을 점검했습니다.

- 타이머 진입 직후 `GetCursorInfo` 기반 I-Beam 판정을 수행하는지 확인
- 시스템 I-Beam이 아니면 `ImeEngine::query` 전에 반환하는지 확인
- 일반·링크·대기·크기 조절·알 수 없는 자체 커서에서 배지를 숨기는지 확인
- I-Beam 영역 재진입 시 전용 강제 갱신 플래그를 설정하되 `old_kind`는 유지해 전환음이 반복되지 않는지 확인
- 배지 표시 경로에서도 I-Beam을 다시 확인하고 알 수 없는 자체 커서 폴백을 사용하지 않는지 확인
- `WM_SETTINGCHANGE`와 `WM_DISPLAYCHANGE` 이후 I-Beam 진입 상태를 재초기화하는지 확인

## 1.0.1 트레이 메뉴 수정

다음 항목을 점검했습니다.

- 기존 `GetCursorPos` + `TrackPopupMenu` 호출을 실제 알림 아이콘 사각형과 모니터 작업 영역을 사용하는 방식으로 교체
- `Shell_NotifyIconGetRect`, `MonitorFromPoint`, `GetMonitorInfoW`, `TrackPopupMenuEx` 선언과 호출 확인
- `TPMPARAMS.rcExclude`로 작업표시줄 영역 또는 숨김 아이콘 팝업의 아이콘 영역을 제외하도록 확인
- 메뉴 표시 동안 숨은 소유 창을 `HWND_TOPMOST`로 올리고 종료 직후 `HWND_NOTOPMOST`로 복원하는 흐름 확인
- `NOTIFYICON_VERSION_4`의 마우스 좌표를 부호 확장해 음수 좌표 보조 모니터를 보존하는지 확인
- 하단, 상단, 왼쪽, 오른쪽 작업표시줄 및 음수 좌표 모니터에 대한 메뉴 배치 회귀 조건 확인
- 알림 아이콘이 숨김 아이콘 팝업 안에 있을 때 아이콘 자체를 제외 영역으로 사용하는 흐름 확인

## 정적 및 데이터 검증

`python tools/static_check.py`를 실행해 다음 항목을 통과했습니다.

- `Cargo.toml`, `Cargo.lock`, 프로그램 버전 `1.0.2` 일치
- 모든 Rust 소스의 문자열, 주석, 괄호 균형
- 커서 6종 각 128바이트, 트레이 아이콘 4종 각 296바이트
- 예제 WAV 3개의 RIFF/WAVE 구조와 유효한 오디오 프레임
- 새 Win32 API, 구조체, 메뉴 배치 함수 포함 여부
- 기존 `TrackPopupMenu` 호출이 메뉴 표시 경로에서 제거되었는지 확인

Clang의 Windows MSVC 대상 레이아웃 검사도 x64와 x86에서 통과했습니다.

- x64: `NOTIFYICONIDENTIFIER` 40바이트, `MONITORINFO` 40바이트, `TPMPARAMS` 20바이트
- x86: `NOTIFYICONIDENTIFIER` 28바이트, `MONITORINFO` 40바이트, `TPMPARAMS` 20바이트
- 기존 구조체 검사: x64 `GUITHREADINFO` 72, `CURSORINFO` 24, `NOTIFYICONDATAW` 976; x86 `GUITHREADINFO` 48, `CURSORINFO` 20, `NOTIFYICONDATAW` 956

## 실행 환경 제한

현재 제작 환경은 Linux이며 Rust 컴파일러와 Windows GUI 세션이 제공되지 않았습니다. 따라서 이 환경에서는 `cargo test`, Windows 실행 파일 빌드 및 실제 포인터 전환/IME 표시 시험을 수행하지 못했습니다. 대신 의존성 없는 정적 검사로 소스 구조, 버전, 자산, I-Beam 선행 게이트와 트레이 메뉴 회귀 조건을 점검했습니다. Windows에서 `build.cmd`를 실행하면 `cargo test` 후 Release 실행 파일을 생성합니다.
