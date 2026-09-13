' ============================================================
'  Cloud Kernel - Gateway Watchdog (hidden)
'  Keeps npb-gateway.exe alive: if it exits or crashes, restart
'  within ~10 seconds. Runs invisibly (no window).
'  Single-instance safe: double-launching will not stack.
' ============================================================
Option Explicit

Dim fso, sh, base, exePath, uiDir
Set fso = CreateObject("Scripting.FileSystemObject")
Set sh  = CreateObject("WScript.Shell")

base    = fso.GetParentFolderName(WScript.ScriptFullName)
exePath = base & "\npb-gateway.exe"
uiDir   = base & "\ui"

If Not fso.FileExists(exePath) Then
    WScript.Quit 1
End If

' ---- single instance guard: count wscript processes running watchdog.vbs ----
Dim wmi, col, p, cnt
Set wmi = GetObject("winmgmts:\\.\root\cimv2")
Set col = wmi.ExecQuery("SELECT ProcessId, CommandLine FROM Win32_Process WHERE Name='wscript.exe'")
cnt = 0
For Each p In col
    If Not IsNull(p.CommandLine) Then
        If InStr(LCase(p.CommandLine), "watchdog.vbs") > 0 Then cnt = cnt + 1
    End If
Next
If cnt > 1 Then
    WScript.Quit 0   ' another watchdog already running
End If

' ---- main loop ----
Dim running, i
Do
    running = False
    Set col = wmi.ExecQuery("SELECT ProcessId FROM Win32_Process WHERE Name='npb-gateway.exe'")
    If col.Count > 0 Then running = True

    If Not running Then
        ' 0 = hidden window, False = do not wait
        sh.Run """" & exePath & """ 0.0.0.0:3000 --ui """ & uiDir & """", 0, False
        WScript.Sleep 4000      ' give it time to bind before next check
    End If

    WScript.Sleep 10000
Loop
