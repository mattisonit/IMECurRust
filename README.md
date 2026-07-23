# IMECurRust

Windows의 현재 입력기 상태를 읽어 **실제로 입력 가능한 텍스트 커서(I-Beam)에만 영문/한글/일본어 표시를 넣는 Rust 프로그램**입니다. 일반 화살표, 손가락, 대기, 크기 조절, 프로그램 자체 커서 및 읽기 전용 텍스트 상태에서는 IME 커서를 적용하지 않습니다.

기존 `IMECur_improved.ahk`의 핵심 동작을 Rust와 Win32 API로 옮겼으며, 외부 Rust 크레이트 없이 동작하도록 구성했습니다.

## 1.0.5 변경 사항

- 회색 인라인 코드와 코드 블록처럼 선택만 가능한 `code`/`pre` 요소를 프로그램 I-Beam 적용 대상에서 제외
- UI Automation의 `ValuePattern` 또는 `TextEditPattern`만 노출되는 요소를 입력 가능으로 간주하던 느슨한 판정 제거
- 브라우저 계열 입력 요소는 키보드 포커스가 가능한 `Edit`, `textbox`, `searchbox`, `spinbutton` 역할일 때만 입력 가능으로 판정
- 일반 `Document`는 키보드 포커스와 `TextEditPattern`이 모두 확인된 `contenteditable` 표면만 허용
- 코드 역할의 자식 요소를 발견한 경우 상위 브라우저 문서의 광범위한 편집 패턴이 판정을 덮어쓰지 못하도록 차단
- UI Automation `AriaRole` 문자열을 BSTR로 안전하게 읽어 코드 표시 요소와 실제 입력 요소를 구분

## 1.0.4 변경 사항

- 수신 이메일 본문과 일반 웹 콘텐츠처럼 선택만 가능한 I-Beam 영역을 프로그램 커서 적용 대상에서 제외
- 편집 가능성이 명확히 확인된 요소에만 `가/A/J` 커서 적용
- UI Automation 조회 실패, 미지원 또는 모호한 결과(`Unknown`)는 안전하게 읽기 전용으로 취급
- UI Automation `Edit` 컨트롤은 `ValuePattern`이 없어도 활성·포커스 가능하고 읽기 전용 표시가 없으면 입력 가능으로 판정
- 브라우저 입력창, `contenteditable`, WPF/WinUI 편집 컨트롤은 `Value.IsReadOnly=false`, `TextEditPattern` 또는 `Edit` 형식으로 계속 지원
- 비편집 I-Beam 영역 진입 시 Windows 커서 구성표를 복원해 기본 I-Beam 표시

## 1.0.3 변경 사항

- 텍스트 선택만 가능하고 입력할 수 없는 읽기 전용 영역에서는 `가/A/J` 커서를 적용하지 않음
- 표준 Win32 `Edit`/`RichEdit`의 `ES_READONLY` 스타일 확인
- Scintilla 편집기의 `SCI_GETREADONLY` 상태 확인
- 브라우저, Electron, WPF, WinUI 등은 UI Automation의 `Value.IsReadOnly`, `TextEdit` 패턴, 컨트롤 형식과 접근성 상태를 이용해 판별
- 읽기 전용 영역 진입 시 `SPI_SETCURSORS`로 사용자의 실제 Windows 커서 구성표를 즉시 복원
- UI Automation 기본값은 실제 속성으로 오인하지 않으며, 최종 판별이 모호하면 읽기 전용으로 처리
- 같은 위치의 접근성 조회 결과를 175ms 동안 캐시해 50ms 타이머의 불필요한 교차 프로세스 호출 감소

## 1.0.2 변경 사항

- 현재 마우스가 Windows 시스템 I-Beam이고 해당 위치가 입력 가능할 때만 IME 상태 조회와 표시 수행
- 일반 화살표, 링크 손가락, 대기, 크기 조절 및 알 수 없는 자체 커서에서는 즉시 처리 중단
- I-Beam 영역에 다시 진입하면 IME 상태를 새로 조회하고 커서를 강제로 갱신
- 마우스가 입력 영역을 벗어나면 호환 배지를 즉시 숨기며, 단순 재진입만으로 전환음을 반복 재생하지 않음
- 자체 커서 앱을 포커스만으로 추정해 배지를 표시하던 느슨한 폴백 제거

## 1.0.1 변경 사항

- 트레이 아이콘 우클릭 메뉴가 작업표시줄 아래에 가려지던 문제 수정
- 실제 알림 아이콘의 화면 사각형을 기준으로 메뉴 위치 계산
- 각 모니터의 작업 영역을 사용해 하단·상단·왼쪽·오른쪽 작업표시줄에 대응
- 보조 모니터의 음수 좌표와 키보드로 연 트레이 메뉴 위치 처리
- 메뉴가 열려 있는 동안에만 숨은 소유 창을 최상위로 전환해 전체 화면 앱 이후의 가림 현상 완화

## 주요 기능

- 포커스 창, 포커스 컨트롤, 캐럿 창, 활성 창을 함께 검사해 실제 입력 대상 탐색
- 선택적으로 마우스 포인터 아래의 가장 깊은 자식 컨트롤을 기준으로 상태 확인
- 한글, 영문 소문자/대문자, 일본어 히라가나/가타카나 I-Beam 표시
- 시스템 I-Beam 교체에 실패했을 때만 사용하는 클릭 통과형 마우스 옆 배지
- 읽기 전용 또는 편집 가능성을 확인할 수 없는 텍스트에서는 배지와 사용자 정의 I-Beam을 모두 숨기고 Windows 기본 I-Beam 복원
- IME 대상 프로그램이 응답하지 않아도 멈추지 않도록 50ms 메시지 제한 적용
- 포커스가 바뀌는 짧은 순간에는 최근 정상 상태를 최대 400ms 재사용
- 상태를 읽지 못하는 상황이 지속되면 오래된 한/영 표시를 자동으로 제거
- 트레이 아이콘 상태 표시, 설정 창, 소리 켜기/끄기, 단일 인스턴스 실행
- Explorer 재시작 후 트레이 아이콘 자동 복구
- 작업표시줄 위치와 모니터 작업 영역을 반영한 트레이 메뉴 배치
- 정상 종료, 로그오프, 패닉 시 Windows 커서 구성표 복원
- 32비트와 64비트 Win32 구조체 및 핸들 크기 대응
- 다중 모니터와 음수 가상 화면 좌표 대응

## 지원 환경

- Windows 10 또는 Windows 11
- Rust 1.70 이상
- 기본 권장 대상: `x86_64-pc-windows-msvc`
- MSVC C++ 빌드 도구가 설치된 Visual Studio Build Tools

이 프로젝트는 Windows 데스크톱용입니다. Linux나 macOS에서는 안내 메시지만 출력합니다.

## 가장 쉬운 빌드 방법

프로젝트 폴더에서 `build.cmd`를 실행합니다.

```bat
build.cmd
```

PowerShell에서 직접 실행할 수도 있습니다.

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\build.ps1
```

스크립트는 다음 작업을 수행합니다.

1. `cargo test`
2. `cargo build --release`
3. `dist` 폴더에 실행 파일, 설정 파일, 알림음 복사

완성된 파일:

```text
dist\IMECurRust.exe
```

테스트 실행을 생략하려면 다음과 같이 실행합니다.

```powershell
.\build.ps1 -SkipTests
```

Cargo 명령을 직접 사용해도 됩니다.

```powershell
cargo test
cargo build --release
```

직접 빌드한 실행 파일은 `target\release\ime-cursor.exe`에 생성됩니다.

## 실행 및 사용

1. `IMECurRust.exe`, `IMECur.ini`, 필요한 WAV 파일을 같은 폴더에 둡니다.
2. `IMECurRust.exe`를 실행합니다.
3. 알림 영역의 아이콘을 우클릭해 **설정**, **정보**, **종료**를 선택합니다.
4. 트레이 아이콘을 두 번 클릭하면 전체 알림음을 빠르게 켜거나 끌 수 있습니다.
5. 프로그램을 한 번 더 실행하면 기존 인스턴스의 설정 창이 열립니다.

프로그램은 콘솔 창 없이 알림 영역에서 실행됩니다.

## 설정 파일

기존 AutoHotkey 버전과 같은 이름과 키를 사용합니다.

```text
IMECur.ini
```

기본 설정:

```ini
[Settings]
GetIMEStatus=1
ShowEnglishIBeam=1
ShowJapaneseIBeam=1
ShowKoreanIBeam=1
ShowFallbackBadge=1
PlayEnglishSound=1
PlayJapaneseSound=1
PlayKoreanSound=1
ShowIMETrayIcon=1
PlaySounds=1
```

| 키 | 의미 | 값 |
|---|---|---|
| `GetIMEStatus` | 상태 확인 기준 | `1`: 포커스/캐럿, `2`: 마우스 아래 컨트롤 |
| `ShowEnglishIBeam` | 영문 I-Beam 표시 | `0` 또는 `1` |
| `ShowJapaneseIBeam` | 일본어 I-Beam 표시 | `0` 또는 `1` |
| `ShowKoreanIBeam` | 한글 I-Beam 표시 | `0` 또는 `1` |
| `ShowFallbackBadge` | 시스템 I-Beam 커서 적용 실패 시 보조 배지 표시 | `0` 또는 `1` |
| `PlayEnglishSound` | 영문 전환음 | `0` 또는 `1` |
| `PlayJapaneseSound` | 일본어 전환음 | `0` 또는 `1` |
| `PlayKoreanSound` | 한글 전환음 | `0` 또는 `1` |
| `ShowIMETrayIcon` | 트레이 아이콘에 상태 표시 | `0` 또는 `1` |
| `PlaySounds` | 전체 소리 사용 | `0` 또는 `1` |

설정 창에서 확인을 누르면 즉시 저장됩니다. 설정 파일을 저장할 수 있도록 실행 폴더에 쓰기 권한이 있어야 합니다.

## 알림음

실행 파일과 같은 폴더에서 다음 파일을 찾습니다.

```text
IMEE.wav   영문
IMEJ.wav   일본어
IMEK.wav   한글
```

`assets` 폴더에는 바로 사용할 수 있는 간단한 예제 알림음이 들어 있습니다. 원하는 WAV 파일로 교체해도 됩니다. 파일이 없으면 해당 상태 전환은 소리 없이 처리됩니다.

## 관리자 권한 프로그램

일반 권한으로 실행된 IMECurRust는 관리자 권한으로 실행된 프로그램의 입력 창 정보를 읽지 못할 수 있습니다. 특정 관리자 프로그램에서만 표시가 나오지 않는 경우 IMECurRust도 같은 권한 수준으로 실행하십시오.

## 비정상 종료 후 커서 복원

정상 종료하면 Windows 마우스 포인터 구성표를 자동으로 다시 불러옵니다. 작업 관리자에서 프로세스를 강제 종료하거나 시스템이 비정상적으로 종료되면 변경된 I-Beam이 남을 수 있습니다.

그 경우 다음 파일을 실행합니다.

```text
restore-cursor.cmd
```

또는 Windows 설정에서 현재 마우스 포인터 구성표를 다시 적용합니다.

## 알려진 제한

- 일부 게임, 원격 데스크톱, 샌드박스, 보안 입력창, 자체 입력 프레임워크는 표준 IMM 상태를 노출하지 않습니다.
- Chromium/Electron 등에서 입력 영역이 자체 커서를 사용하거나 UI Automation으로 편집 가능성을 확인할 수 없으면 표시하지 않습니다.
- 접근성 공급자가 편집 가능 여부를 노출하지 않는 비표준 앱은 오탐 방지를 위해 기존 I-Beam 전용 동작을 유지할 수 있습니다.
- 전역 시스템 I-Beam을 교체하므로 다른 커서 변경 프로그램과 동시에 사용하면 서로의 설정을 덮어쓸 수 있습니다.
- 중국어 입력기는 오판을 피하기 위해 중립 상태로 처리합니다.

## 프로젝트 구조

```text
IMECurRust/
├─ Cargo.toml
├─ Cargo.lock
├─ IMECur.ini
├─ build.cmd
├─ build.ps1
├─ restore-cursor.cmd
├─ restore-cursor.ps1
├─ assets/
│  ├─ IMEE.wav
│  ├─ IMEJ.wav
│  └─ IMEK.wav
├─ tools/
│  └─ static_check.py  의존성 없는 정적 검증
└─ src/
   ├─ main.rs       진입점, 창/트레이/설정/커서/배지 처리
   ├─ editability.rs 읽기 전용/입력 가능 판별과 UI Automation 처리
   ├─ ime.rs        입력 대상 탐색, IME 조회, 상태 캐시
   ├─ config.rs     기존 INI 호환 읽기/쓰기
   ├─ assets.rs     커서 마스크와 트레이 아이콘 데이터
   └─ win.rs        필요한 Win32 FFI 선언
```

## 구현상 중요한 점

- `GetCursorInfo`의 현재 커서가 시스템 `IDC_IBEAM`과 일치한 뒤, `EditabilityDetector`가 해당 위치를 읽기 전용이 아니라고 판단한 경우에만 IME 엔진을 호출합니다.
- 표준 `Edit`/`RichEdit`는 `ES_READONLY`, Scintilla는 `SCI_GETREADONLY`, 비표준 UI는 UI Automation의 `Value.IsReadOnly` 및 `TextEdit` 패턴으로 판별합니다.
- 읽기 전용 진입 시 `SystemParametersInfoW(SPI_SETCURSORS, ...)`로 실제 Windows 커서 구성표를 복원합니다.
- `GUITHREADINFO`, `CURSORINFO`, `NOTIFYICONDATAW`는 `size_of`로 실제 플랫폼 크기를 사용합니다.
- `ImmGetDefaultIMEWnd`로 후보 창의 기본 IME 창을 찾고 `WM_IME_CONTROL`로 열림 상태와 변환 모드를 조회합니다.
- `SendMessageTimeoutW`를 사용해 응답 없는 대상에서 프로그램 전체가 멈추지 않게 합니다.
- `SetSystemCursor` 성공 시 Windows가 전달된 커서 핸들의 소유권을 가져가므로 성공한 핸들을 다시 파기하지 않습니다.
- 트레이 메뉴는 `Shell_NotifyIconGetRect`로 아이콘 위치를 구하고 `GetMonitorInfoW`의 작업 영역 경계에서 `TrackPopupMenuEx`로 표시합니다.
- 메뉴 표시 중에는 숨은 소유 창만 임시로 최상위로 올리고 메뉴 종료 즉시 원래 상태로 되돌립니다.
- 종료 시 `SystemParametersInfoW(SPI_SETCURSORS, ...)`로 사용자의 Windows 커서 구성표를 다시 불러옵니다.

## 라이선스

MIT License. 자세한 내용은 `LICENSE`를 참조하세요.
