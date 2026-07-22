$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class ImeCursorReset
{
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SystemParametersInfo(
        uint uiAction,
        uint uiParam,
        IntPtr pvParam,
        uint fWinIni);
}
"@

$SPI_SETCURSORS = 0x0057
if (-not [ImeCursorReset]::SystemParametersInfo($SPI_SETCURSORS, 0, [IntPtr]::Zero, 0)) {
    $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    throw "Windows 커서를 복원하지 못했습니다. Win32 오류 코드: $errorCode"
}

Write-Host "Windows 마우스 포인터 구성표를 다시 불러왔습니다."
