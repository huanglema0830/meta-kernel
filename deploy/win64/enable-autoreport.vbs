' ============================================================
'  Enable AUTO-REPORT on this machine (the old notebook).
'  Run this ONCE. After that, every logon will automatically:
'     1) scan the LAN  -> find the Cloud Kernel gateway
'     2) sample the field -> report back
'  No admin required. No IP or port needed.
' ============================================================
Option Explicit

Dim fso, sh, base, runner
Set fso = CreateObject("Scripting.FileSystemObject")
Set sh  = CreateObject("WScript.Shell")

base   = fso.GetParentFolderName(WScript.ScriptFullName)
runner = base & "\autoprobe-hidden.vbs"

If Not fso.FileExists(runner) Then
    WScript.Echo "ERROR: autoprobe-hidden.vbs not found next to this script."
    WScript.Quit 1
End If

' ---- register for current user (no elevation) ----
sh.RegWrite "HKCU\Software\Microsoft\Windows\CurrentVersion\Run\CloudKernelAutoProbe", _
            "wscript.exe """ & runner & """", "REG_SZ"

' ---- run once right now (hidden) to verify ----
sh.Run "wscript.exe """ & runner & """", 0, False

WScript.Echo "OK - auto-report ENABLED on this machine." & vbCrLf & vbCrLf & _
             "From now on, each time you log in, this PC will automatically" & vbCrLf & _
             "find the gateway on the LAN and report its field data." & vbCrLf & _
             "You do not need to do anything else." & vbCrLf & vbCrLf & _
             "Registered:" & vbCrLf & "  " & runner
