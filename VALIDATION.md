# 검증 기록

## 1.0.5 인라인 코드 오탐 수정

다음 항목을 점검했습니다.

- UI Automation `AriaRole=code`, `doc-code`, `pre` 요소가 `CodeLikeText`로 분류되는지 확인
- 코드 역할 요소가 `ValuePattern` 또는 `TextEditPattern`을 잘못 노출해도 입력 가능으로 판정하지 않는지 확인
- 코드 역할 자식을 확인한 뒤에는 상위 `Document`의 편집 패턴이 읽기 전용 판정을 덮어쓰지 않는지 확인
- 키보드 포커스 가능한 UIA `Edit` 및 ARIA `textbox`/`searchbox` 입력 요소는 계속 입력 가능으로 판정되는지 확인
- 일반 브라우저 `Document`는 `TextEditPattern=true`와 `IsKeyboardFocusable=true`가 모두 확인된 경우에만 편집 가능으로 판정되는지 확인
- UI Automation BSTR 속성을 `SysStringLen`으로 읽고 `VariantClear`로 해제하는지 확인
- `ValuePattern`/`TextEditPattern`만 있는 사용자 정의 요소는 안전하게 `Unknown`으로 유지되어 Windows 기본 I-Beam을 사용하는지 확인

## 1.0.4 브라우저·이메일 본문 오탐 수정

다음 항목을 점검했습니다.

- 사용자 정의 IME 커서는 `Editability::Editable`일 때만 허용하는지 확인
- `Editability::ReadOnly`뿐 아니라 `Editability::Unknown`도 IME 조회 전에 차단하는지 확인
- UI Automation 실패·미지원·모호한 웹 문서가 Windows 기본 I-Beam으로 복원되는지 확인
- UI Automation `Text`/`Document` 요소가 명시적 편집 부모를 찾지 못하면 읽기 전용으로 판정되는지 확인
- `Value.IsReadOnly=false`, `TextEditPattern=true`, 활성 `Edit` 컨트롤은 입력 가능으로 유지되는지 확인
- 비편집 영역에서 입력 영역으로 이동하면 강제 갱신 플래그를 설정해 IME 상태를 즉시 다시 읽는지 확인
- `Unknown`을 허용하던 기존 경로가 제거되어 수신 이메일 본문과 일반 웹 콘텐츠 오탐을 방지하는지 확인

## 1.0.3 읽기 전용 텍스트 제외 수정

다음 항목을 점검했습니다.

- I-Beam 판정 이후, IME 조회 이전에 `EditabilityDetector::at_cursor`가 실행되는지 확인
- 읽기 전용 판정 시 IME 엔진 호출과 배지 표시 없이 즉시 반환하는지 확인
- 읽기 전용 진입 시 `SystemParametersInfoW(SPI_SETCURSORS, ...)`로 Windows 커서 구성표를 복원하는지 확인
- 복원 실패 시 1초 간격으로만 재시도해 50ms 타이머에서 API를 반복 호출하지 않는지 확인
- 표준 `Edit`/`RichEdit`의 `ES_READONLY`, Scintilla의 `SCI_GETREADONLY` 검사 포함 여부 확인
- UI Automation에서 `Value.IsReadOnly`, `TextEdit`, `ControlType`, Legacy 접근성 상태를 검사하는지 확인
- `GetCurrentPropertyValueEx`의 `ignoreDefaultValue=TRUE` 경로를 사용해 미지원 속성의 기본값을 실제 상태로 오인하지 않는지 확인
- 텍스트 자식에서 편집 가능한 부모까지 최대 6단계 탐색해 `contenteditable` 계열을 읽기 전용으로 오판하지 않도록 구성했는지 확인
- UI Automation 및 표준 컨트롤 판별 결과를 같은 위치에서 175ms 캐시하는지 확인
- 1.0.3에서는 알 수 없는 비표준 프레임워크를 허용했으나, 1.0.4에서 이 정책을 안전한 기본 거부로 변경

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

- `Cargo.toml`, `Cargo.lock`, 프로그램 버전 `1.0.5` 일치
- 모든 Rust 소스의 문자열, 주석, 괄호 균형
- 커서 6종 각 128바이트, 트레이 아이콘 4종 각 296바이트
- 예제 WAV 3개의 RIFF/WAVE 구조와 유효한 오디오 프레임
- 새 Win32/UI Automation API, `VARIANT` 구조체, 읽기 전용 판별 및 메뉴 배치 함수 포함 여부
- 기존 `TrackPopupMenu` 호출이 메뉴 표시 경로에서 제거되었는지 확인

Clang의 Windows MSVC 대상 레이아웃 검사도 x64와 x86에서 통과했습니다.

- x64: `NOTIFYICONIDENTIFIER` 40바이트, `MONITORINFO` 40바이트, `TPMPARAMS` 20바이트
- x86: `NOTIFYICONIDENTIFIER` 28바이트, `MONITORINFO` 40바이트, `TPMPARAMS` 20바이트
- UI Automation `VARIANT`: x64 24바이트, x86 16바이트
- 기존 구조체 검사: x64 `GUITHREADINFO` 72, `CURSORINFO` 24, `NOTIFYICONDATAW` 976; x86 `GUITHREADINFO` 48, `CURSORINFO` 20, `NOTIFYICONDATAW` 956

## 실행 환경 제한

현재 제작 환경에는 Rust 컴파일러와 Windows GUI 세션이 제공되지 않았습니다. 따라서 실제 Windows 앱별 UI Automation 공급자, 포인터 전환 및 IME 표시 동작은 이 환경에서 시험하지 못했습니다. 의존성 없는 정적 검사와 x86/x64 ABI 레이아웃 검사로 소스 구조, 버전, 자산, I-Beam·읽기 전용 선행 게이트와 트레이 메뉴 회귀 조건을 점검했습니다. Windows에서 `build.cmd`를 실행하면 `cargo test` 후 Release 실행 파일을 생성합니다.
