' ============================================================
'  Enable auto-start (current user, NO admin required)
'  Registers: HKCU\...\CurrentVersion\Run\CloudKernelGateway
'  Then launches the watchdog immediately.
' ============================================================
Option Explicit

Dim fso, sh, base, wd
Set fso = CreateObject("Scripting.FileSystemObject")
Set sh  = CreateObject("WScript.Shell")

base = fso.GetParentFolderName(WScript.ScriptFullName)
wd   = base & "\watchdog.vbs"

If Not fso.FileExists(wd) Then
    WScript.Echo "ERROR: watchdog.vbs not found next to this script." & vbCrLf & base
    WScript.Quit 1
End If

' ---- register for current user (no elevation needed) ----
sh.RegWrite "HKCU\Software\Microsoft\Windows\CurrentVersion\Run\CloudKernelGateway", """" & wd & """", "REG_SZ"

' ---- start right now (hidden) ----
sh.Run """" & wd & """", 0, False

WScript.Echo "OK - auto-start enabled." & vbCrLf & vbCrLf & _
             "The ZhengYuan OS gateway will now:" & vbCrLf & _
             "  1) start automatically every time you log in" & vbCrLf & _
             "  2) auto-restart within ~10s if it ever crashes or is closed" & vbCrLf & vbCrLf & _
             "Registered path:" & vbCrLf & "  " & wd
